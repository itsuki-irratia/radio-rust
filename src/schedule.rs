use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use gstreamer as gst;
use rusqlite::types::Type;
use rusqlite::{Connection, Row, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::playback::{
    canonical_playback_source, expand_playback_sources, is_xspf_source, play_file_with_fades_from,
};
use crate::types::{SUPPORTED_EXTENSIONS, ScanResult, ScheduleDb, ScheduleEntry};

pub fn run_scan(folder: &Path, json: bool) -> Result<()> {
    if !folder.exists() {
        bail!("Folder does not exist: {}", folder.display());
    }
    if !folder.is_dir() {
        bail!("Path is not a directory: {}", folder.display());
    }

    let mut files = Vec::new();
    collect_media_files(folder, &mut files)?;
    files.sort();

    if json {
        let result = ScanResult {
            folder: folder
                .canonicalize()
                .context("Failed to canonicalize folder path")?
                .display()
                .to_string(),
            files: files
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&result).context("Failed to serialize JSON output")?
        );
    } else {
        for path in files {
            println!("{}", path.display());
        }
    }

    Ok(())
}

pub fn run_schedule_add(
    db_path: &Path,
    file: &Path,
    at: &str,
    fade_in: u64,
    fade_out: u64,
    volume: f64,
    mute: bool,
) -> Result<()> {
    let source = file.display().to_string();
    let canonical_source = canonical_playback_source(&source)?;
    if !is_remote_media_source(&canonical_source) && !is_supported_media_file(file) {
        bail!("Unsupported media extension for {}", file.display());
    }
    if is_xspf_source(&canonical_source) {
        expand_playback_sources(&canonical_source)?;
    }
    validate_volume(volume)?;

    let at_dt = parse_scheduled_datetime(at)?;
    let conn = open_schedule_db(db_path)?;
    let entry = ScheduleEntry {
        id: 0,
        file: canonical_source.clone(),
        at: at_dt,
        fade_in_secs: fade_in,
        fade_out_secs: fade_out,
        volume,
        mute,
    };
    let next_id = insert_new_schedule_entry(&conn, &entry)?;

    println!(
        "Added #{} at {} | fade-in {}s | fade-out {}s | volume {:.2} | mute {} | {}",
        next_id,
        at_dt.to_rfc3339(),
        fade_in,
        fade_out,
        volume,
        mute,
        canonical_source
    );
    Ok(())
}

pub fn run_schedule_list(
    db_path: &Path,
    json: bool,
    day: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<()> {
    let date_filter = parse_schedule_date_filter(day, from, to)?;
    crate::cron::sync_cron_schedule(db_path)?;
    let mut db = load_schedule(db_path)?;
    sort_schedule_entries(&mut db.entries);
    if let Some(filter) = date_filter {
        db.entries
            .retain(|entry| filter.matches(entry.at.date_naive()));
    }

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&db).context("Failed to serialize schedule JSON")?
        );
        return Ok(());
    }

    if db.entries.is_empty() {
        if let Some(filter) = date_filter {
            println!(
                "No scheduled items for {} in {}",
                filter.description(),
                db_path.display()
            );
        } else {
            println!("No scheduled items in {}", db_path.display());
        }
        return Ok(());
    }

    for entry in db.entries {
        println!(
            "#{} | {} | fade-in {}s | fade-out {}s | volume {:.2} | mute {} | {}",
            entry.id,
            entry.at.to_rfc3339(),
            entry.fade_in_secs,
            entry.fade_out_secs,
            entry.volume,
            entry.mute,
            entry.file
        );
    }
    Ok(())
}

pub fn run_schedule_run(db_path: &Path) -> Result<()> {
    gst::init().context("Failed to initialize GStreamer")?;

    loop {
        crate::cron::sync_cron_schedule(db_path)?;
        let mut db = load_schedule(db_path)?;
        sort_schedule_entries(&mut db.entries);

        let Some(next) = db.entries.first().cloned() else {
            println!("Schedule empty in {}. Nothing to run.", db_path.display());
            return Ok(());
        };

        let now = Local::now();
        if next.at > now {
            let wait = (next.at - now)
                .to_std()
                .context("Failed converting schedule delay to std::time::Duration")?;
            println!("Waiting until {} for #{}...", next.at.to_rfc3339(), next.id);
            thread::sleep(wait);
        } else {
            println!(
                "Item #{} is in the past ({}), starting now...",
                next.id,
                next.at.to_rfc3339()
            );
        }

        let start_offset = (Local::now() - next.at).to_std().unwrap_or_default();
        let file = PathBuf::from(&next.file);
        play_file_with_fades_from(
            &file,
            next.fade_in_secs,
            next.fade_out_secs,
            next.volume,
            next.mute,
            start_offset,
        )?;

        remove_schedule_entry(db_path, next.id)?;
        println!("Completed and removed #{}", next.id);
    }
}

pub fn load_schedule(db_path: &Path) -> Result<ScheduleDb> {
    let conn = open_schedule_db(db_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, file, at_rfc3339, fade_in_secs, fade_out_secs, volume, mute
             FROM schedule_entries
             ORDER BY at_unix_ms ASC, id ASC",
        )
        .context("Failed to prepare schedule query")?;
    let entries = stmt
        .query_map([], schedule_entry_from_row)
        .context("Failed to query schedule entries")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read schedule entries")?;
    Ok(ScheduleDb { entries })
}

pub fn remove_schedule_entry(db_path: &Path, id: u64) -> Result<()> {
    let conn = open_schedule_db(db_path)?;
    let id = i64::try_from(id).context("Schedule id is too large for SQLite")?;
    conn.execute("DELETE FROM schedule_entries WHERE id = ?1", params![id])
        .context("Failed to remove schedule entry")?;
    Ok(())
}

pub fn open_schedule_db(db_path: &Path) -> Result<Connection> {
    if let Some(parent) = db_path.parent().filter(|path| !path.as_os_str().is_empty()) {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create schedule database directory {}",
                parent.display()
            )
        })?;
    }

    let db_existed = db_path.exists();
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open schedule database {}", db_path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))
        .context("Failed to configure schedule database busy timeout")?;
    init_schedule_schema(&conn)?;
    if !db_existed {
        import_legacy_json_schedule(db_path, &conn)?;
    }
    Ok(conn)
}

fn init_schedule_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schedule_entries (
            id INTEGER PRIMARY KEY,
            file TEXT NOT NULL,
            at_unix_ms INTEGER NOT NULL,
            at_rfc3339 TEXT NOT NULL,
            fade_in_secs INTEGER NOT NULL,
            fade_out_secs INTEGER NOT NULL,
            volume REAL NOT NULL DEFAULT 1.0,
            mute INTEGER NOT NULL DEFAULT 0 CHECK (mute IN (0, 1)),
            cron_id INTEGER,
            cron_at_unix_ms INTEGER
        );
        CREATE INDEX IF NOT EXISTS schedule_entries_at_idx
            ON schedule_entries (at_unix_ms, id);
        ",
    )
    .context("Failed to initialize schedule database schema")?;
    add_schedule_column_if_missing(conn, "cron_id", "INTEGER")?;
    add_schedule_column_if_missing(conn, "cron_at_unix_ms", "INTEGER")?;
    conn.execute_batch(
        "
        CREATE UNIQUE INDEX IF NOT EXISTS schedule_entries_cron_occurrence_idx
            ON schedule_entries (cron_id, cron_at_unix_ms)
            WHERE cron_id IS NOT NULL AND cron_at_unix_ms IS NOT NULL;
        ",
    )
    .context("Failed to initialize schedule cron occurrence index")?;
    Ok(())
}

fn add_schedule_column_if_missing(conn: &Connection, name: &str, data_type: &str) -> Result<()> {
    let sql = format!("ALTER TABLE schedule_entries ADD COLUMN {name} {data_type}");
    match conn.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(error) => Err(error).with_context(|| format!("Failed to add schedule column {name}")),
    }
}

fn import_legacy_json_schedule(db_path: &Path, conn: &Connection) -> Result<()> {
    let legacy_path = db_path.with_extension("json");
    if !legacy_path.exists() {
        return Ok(());
    }

    let raw = fs::read_to_string(&legacy_path).with_context(|| {
        format!(
            "Failed to read legacy schedule file {}",
            legacy_path.display()
        )
    })?;
    if raw.trim().is_empty() {
        return Ok(());
    }

    let mut db: ScheduleDb = serde_json::from_str(&raw).with_context(|| {
        format!(
            "Failed to parse legacy schedule file {}",
            legacy_path.display()
        )
    })?;
    sort_schedule_entries(&mut db.entries);
    if db.entries.is_empty() {
        return Ok(());
    }

    for entry in &db.entries {
        insert_schedule_entry(conn, entry)?;
    }
    eprintln!(
        "Imported {} scheduled item(s) from legacy JSON {} into {}",
        db.entries.len(),
        legacy_path.display(),
        db_path.display()
    );
    Ok(())
}

fn insert_schedule_entry(conn: &Connection, entry: &ScheduleEntry) -> Result<()> {
    let id = i64::try_from(entry.id).context("Schedule id is too large for SQLite")?;
    let fade_in_secs =
        i64::try_from(entry.fade_in_secs).context("Fade-in seconds are too large for SQLite")?;
    let fade_out_secs =
        i64::try_from(entry.fade_out_secs).context("Fade-out seconds are too large for SQLite")?;
    conn.execute(
        "INSERT INTO schedule_entries
            (id, file, at_unix_ms, at_rfc3339, fade_in_secs, fade_out_secs, volume, mute)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            &entry.file,
            entry.at.timestamp_millis(),
            entry.at.to_rfc3339(),
            fade_in_secs,
            fade_out_secs,
            entry.volume,
            if entry.mute { 1 } else { 0 },
        ],
    )
    .with_context(|| format!("Failed to insert schedule entry #{}", entry.id))?;
    Ok(())
}

fn insert_new_schedule_entry(conn: &Connection, entry: &ScheduleEntry) -> Result<u64> {
    let fade_in_secs =
        i64::try_from(entry.fade_in_secs).context("Fade-in seconds are too large for SQLite")?;
    let fade_out_secs =
        i64::try_from(entry.fade_out_secs).context("Fade-out seconds are too large for SQLite")?;
    conn.execute(
        "INSERT INTO schedule_entries
            (file, at_unix_ms, at_rfc3339, fade_in_secs, fade_out_secs, volume, mute)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            &entry.file,
            entry.at.timestamp_millis(),
            entry.at.to_rfc3339(),
            fade_in_secs,
            fade_out_secs,
            entry.volume,
            if entry.mute { 1 } else { 0 },
        ],
    )
    .context("Failed to insert schedule entry")?;

    let id = conn.last_insert_rowid();
    u64::try_from(id).context("SQLite returned an invalid schedule id")
}

fn schedule_entry_from_row(row: &Row<'_>) -> rusqlite::Result<ScheduleEntry> {
    let id: i64 = row.get(0)?;
    let at_text: String = row.get(2)?;
    let at = DateTime::parse_from_rfc3339(&at_text)
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(error)))?
        .with_timezone(&Local);
    let fade_in_secs: i64 = row.get(3)?;
    let fade_out_secs: i64 = row.get(4)?;
    let mute: i64 = row.get(6)?;

    Ok(ScheduleEntry {
        id: u64::try_from(id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, Type::Integer, Box::new(error))
        })?,
        file: row.get(1)?,
        at,
        fade_in_secs: u64::try_from(fade_in_secs).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(3, Type::Integer, Box::new(error))
        })?,
        fade_out_secs: u64::try_from(fade_out_secs).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, Type::Integer, Box::new(error))
        })?,
        volume: row.get(5)?,
        mute: mute != 0,
    })
}

pub fn sort_schedule_entries(entries: &mut [ScheduleEntry]) {
    entries.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
}

pub fn validate_volume(volume: f64) -> Result<()> {
    if !(0.0..=1.0).contains(&volume) {
        bail!("Invalid volume {volume}. Use a value between 0.0 and 1.0");
    }
    Ok(())
}

fn collect_media_files(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("Failed reading directory {}", dir.display()))?;

    for entry in entries {
        let entry =
            entry.with_context(|| format!("Failed reading an entry in {}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            collect_media_files(&path, output)?;
            continue;
        }

        if is_supported_media_file(&path) {
            output.push(path);
        }
    }

    Ok(())
}

fn is_supported_media_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let extension_lower = extension.to_ascii_lowercase();
    SUPPORTED_EXTENSIONS.contains(&extension_lower.as_str())
}

fn is_remote_media_source(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

fn parse_scheduled_datetime(input: &str) -> Result<DateTime<Local>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(input) {
        return Ok(dt.with_timezone(&Local));
    }

    for format in ["%H:%M:%S", "%H:%M"] {
        if let Ok(time) = NaiveTime::parse_from_str(input, format) {
            let naive = Local::now().date_naive().and_time(time);
            return resolve_local_datetime(naive, input);
        }
    }

    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(input, format) {
            return resolve_local_datetime(naive, input);
        }
    }

    bail!(
        "Invalid datetime format: {input}. Use RFC3339 like 2026-05-10T21:30:00+02:00 or local form YYYY-MM-DD HH:MM[:SS]"
    )
}

fn parse_schedule_day(input: &str) -> Result<NaiveDate> {
    if input.eq_ignore_ascii_case("today") {
        return Ok(Local::now().date_naive());
    }

    NaiveDate::parse_from_str(input, "%Y-%m-%d").with_context(|| {
        format!("Invalid day {input}. Use `today` or a local date like 2026-05-10")
    })
}

#[derive(Clone, Copy)]
struct ScheduleDateFilter {
    from: NaiveDate,
    to: NaiveDate,
}

impl ScheduleDateFilter {
    fn matches(self, day: NaiveDate) -> bool {
        self.from <= day && day <= self.to
    }

    fn description(self) -> String {
        if self.from == self.to {
            self.from.to_string()
        } else {
            format!("{} to {}", self.from, self.to)
        }
    }
}

fn parse_schedule_date_filter(
    day: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Option<ScheduleDateFilter>> {
    if day.is_some() && (from.is_some() || to.is_some()) {
        bail!("Use either `--day` or `--from`/`--to`, not both");
    }

    if let Some(day) = day {
        let day = parse_schedule_day(day)?;
        return Ok(Some(ScheduleDateFilter { from: day, to: day }));
    }

    match (from, to) {
        (None, None) => Ok(None),
        (Some(from), Some(to)) => {
            let from = parse_schedule_day(from)?;
            let to = parse_schedule_day(to)?;
            if from > to {
                bail!("Invalid date range: --from {from} is after --to {to}");
            }
            Ok(Some(ScheduleDateFilter { from, to }))
        }
        (Some(_), None) => bail!("Missing --to for schedule date range"),
        (None, Some(_)) => bail!("Missing --from for schedule date range"),
    }
}

fn resolve_local_datetime(naive: NaiveDateTime, input: &str) -> Result<DateTime<Local>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Ok(dt),
        LocalResult::Ambiguous(a, b) => bail!(
            "Ambiguous local datetime (DST overlap): {} or {}",
            a.to_rfc3339(),
            b.to_rfc3339()
        ),
        LocalResult::None => bail!("Invalid local datetime for your timezone: {input}"),
    }
}
