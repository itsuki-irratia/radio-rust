use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use rusqlite::types::Type;
use rusqlite::{Connection, Row, params};
use std::collections::BTreeSet;
use std::path::Path;

use crate::playback::{canonical_playback_source, expand_playback_sources, is_xspf_source};
use crate::schedule::{open_schedule_db, validate_volume};
use crate::types::{CronDb, CronEntry, SUPPORTED_EXTENSIONS};

const CRON_LOOKBACK_SECS: i64 = 60;
const CRON_LOOKAHEAD_SECS: i64 = 24 * 60 * 60;

pub fn run_cron_add(
    db_path: &Path,
    file: &Path,
    expression: &str,
    fade_in: u64,
    fade_out: u64,
    volume: f64,
    mute: bool,
) -> Result<()> {
    let schedule = CronSchedule::parse(expression)?;
    let source = file.display().to_string();
    let canonical_source = canonical_playback_source(&source)?;
    if !is_remote_media_source(&canonical_source) && !is_supported_media_file(file) {
        bail!("Unsupported media extension for {}", file.display());
    }
    if is_xspf_source(&canonical_source) {
        expand_playback_sources(&canonical_source)?;
    }
    validate_volume(volume)?;

    let conn = open_cron_db(db_path)?;
    let next_id = insert_new_cron_entry(
        &conn,
        expression,
        &canonical_source,
        fade_in,
        fade_out,
        volume,
        mute,
    )?;
    sync_cron_schedule_with_connection(&conn)?;

    println!(
        "Added cron #{} | {} | fade-in {}s | fade-out {}s | volume {:.2} | mute {} | {}",
        next_id, schedule.expression, fade_in, fade_out, volume, mute, canonical_source
    );
    Ok(())
}

pub fn run_cron_list(db_path: &Path, json: bool) -> Result<()> {
    let db = load_cron_entries(db_path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&db).context("Failed to serialize cron JSON")?
        );
        return Ok(());
    }

    if db.entries.is_empty() {
        println!("No cron items in {}", db_path.display());
        return Ok(());
    }

    for entry in db.entries {
        println!(
            "#{} | {} | enabled {} | fade-in {}s | fade-out {}s | volume {:.2} | mute {} | {}",
            entry.id,
            entry.expression,
            entry.enabled,
            entry.fade_in_secs,
            entry.fade_out_secs,
            entry.volume,
            entry.mute,
            entry.file
        );
    }
    Ok(())
}

pub fn run_cron_remove(db_path: &Path, id: u64) -> Result<()> {
    let conn = open_cron_db(db_path)?;
    let id = i64::try_from(id).context("Cron id is too large for SQLite")?;
    let removed = conn
        .execute("DELETE FROM cron_entries WHERE id = ?1", params![id])
        .context("Failed to remove cron entry")?;
    conn.execute(
        "DELETE FROM schedule_entries WHERE cron_id = ?1",
        params![id],
    )
    .context("Failed to remove materialized cron schedule entries")?;
    conn.execute(
        "DELETE FROM cron_runs WHERE cron_id = ?1
         AND at_unix_ms > ?2",
        params![id, Local::now().timestamp_millis()],
    )
    .context("Failed to remove future cron run markers")?;

    if removed == 0 {
        println!("No cron item #{}", id);
    } else {
        println!("Removed cron #{}", id);
    }
    Ok(())
}

pub fn sync_cron_schedule(db_path: &Path) -> Result<()> {
    let conn = open_cron_db(db_path)?;
    sync_cron_schedule_with_connection(&conn)
}

pub fn load_cron_entries(db_path: &Path) -> Result<CronDb> {
    let conn = open_cron_db(db_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, expression, file, fade_in_secs, fade_out_secs, volume, mute, enabled
             FROM cron_entries
             ORDER BY id ASC",
        )
        .context("Failed to prepare cron query")?;
    let entries = stmt
        .query_map([], cron_entry_from_row)
        .context("Failed to query cron entries")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read cron entries")?;
    Ok(CronDb { entries })
}

fn open_cron_db(db_path: &Path) -> Result<Connection> {
    let conn = open_schedule_db(db_path)?;
    init_cron_schema(&conn)?;
    Ok(conn)
}

fn init_cron_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS cron_entries (
            id INTEGER PRIMARY KEY,
            expression TEXT NOT NULL,
            file TEXT NOT NULL,
            fade_in_secs INTEGER NOT NULL,
            fade_out_secs INTEGER NOT NULL,
            volume REAL NOT NULL DEFAULT 1.0,
            mute INTEGER NOT NULL DEFAULT 0 CHECK (mute IN (0, 1)),
            enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
        );
        CREATE TABLE IF NOT EXISTS cron_runs (
            cron_id INTEGER NOT NULL,
            at_unix_ms INTEGER NOT NULL,
            PRIMARY KEY (cron_id, at_unix_ms)
        );
        ",
    )
    .context("Failed to initialize cron database schema")
}

fn sync_cron_schedule_with_connection(conn: &Connection) -> Result<()> {
    init_cron_schema(conn)?;
    let entries = load_enabled_cron_entries(conn)?;
    let now = Local::now();
    let start = now - chrono::Duration::seconds(CRON_LOOKBACK_SECS);
    let end = now + chrono::Duration::seconds(CRON_LOOKAHEAD_SECS);

    for entry in entries {
        let schedule = CronSchedule::parse(&entry.expression)?;
        let mut occurrences = Vec::new();
        if let Some(previous) = schedule.last_between(start, now) {
            occurrences.push(previous);
        }
        if let Some(next) = schedule.next_between(now, end) {
            if !occurrences.iter().any(|occurrence| *occurrence == next) {
                occurrences.push(next);
            }
        }

        for occurrence in occurrences {
            materialize_cron_occurrence(conn, &entry, occurrence)?;
        }
    }
    Ok(())
}

fn load_enabled_cron_entries(conn: &Connection) -> Result<Vec<CronEntry>> {
    let mut stmt = conn
        .prepare(
            "SELECT id, expression, file, fade_in_secs, fade_out_secs, volume, mute, enabled
             FROM cron_entries
             WHERE enabled = 1
             ORDER BY id ASC",
        )
        .context("Failed to prepare enabled cron query")?;
    stmt.query_map([], cron_entry_from_row)
        .context("Failed to query enabled cron entries")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read enabled cron entries")
}

fn materialize_cron_occurrence(
    conn: &Connection,
    entry: &CronEntry,
    at: DateTime<Local>,
) -> Result<()> {
    let cron_id = i64::try_from(entry.id).context("Cron id is too large for SQLite")?;
    let at_unix_ms = at.timestamp_millis();
    let inserted_run = conn
        .execute(
            "INSERT OR IGNORE INTO cron_runs (cron_id, at_unix_ms) VALUES (?1, ?2)",
            params![cron_id, at_unix_ms],
        )
        .context("Failed to insert cron run marker")?;
    if inserted_run == 0 {
        return Ok(());
    }

    let fade_in_secs =
        i64::try_from(entry.fade_in_secs).context("Fade-in seconds are too large for SQLite")?;
    let fade_out_secs =
        i64::try_from(entry.fade_out_secs).context("Fade-out seconds are too large for SQLite")?;
    conn.execute(
        "INSERT INTO schedule_entries
            (file, at_unix_ms, at_rfc3339, fade_in_secs, fade_out_secs, volume, mute, cron_id, cron_at_unix_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            &entry.file,
            at_unix_ms,
            at.to_rfc3339(),
            fade_in_secs,
            fade_out_secs,
            entry.volume,
            if entry.mute { 1 } else { 0 },
            cron_id,
            at_unix_ms,
        ],
    )
    .with_context(|| format!("Failed to materialize cron #{} at {}", entry.id, at.to_rfc3339()))?;
    Ok(())
}

fn insert_new_cron_entry(
    conn: &Connection,
    expression: &str,
    file: &str,
    fade_in: u64,
    fade_out: u64,
    volume: f64,
    mute: bool,
) -> Result<u64> {
    let fade_in = i64::try_from(fade_in).context("Fade-in seconds are too large for SQLite")?;
    let fade_out = i64::try_from(fade_out).context("Fade-out seconds are too large for SQLite")?;
    conn.execute(
        "INSERT INTO cron_entries
            (expression, file, fade_in_secs, fade_out_secs, volume, mute, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
        params![
            expression,
            file,
            fade_in,
            fade_out,
            volume,
            if mute { 1 } else { 0 },
        ],
    )
    .context("Failed to insert cron entry")?;

    let id = conn.last_insert_rowid();
    u64::try_from(id).context("SQLite returned an invalid cron id")
}

fn cron_entry_from_row(row: &Row<'_>) -> rusqlite::Result<CronEntry> {
    let id: i64 = row.get(0)?;
    let fade_in_secs: i64 = row.get(3)?;
    let fade_out_secs: i64 = row.get(4)?;
    let mute: i64 = row.get(6)?;
    let enabled: i64 = row.get(7)?;

    Ok(CronEntry {
        id: u64::try_from(id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, Type::Integer, Box::new(error))
        })?,
        expression: row.get(1)?,
        file: row.get(2)?,
        fade_in_secs: u64::try_from(fade_in_secs).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(3, Type::Integer, Box::new(error))
        })?,
        fade_out_secs: u64::try_from(fade_out_secs).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, Type::Integer, Box::new(error))
        })?,
        volume: row.get(5)?,
        mute: mute != 0,
        enabled: enabled != 0,
    })
}

#[derive(Debug)]
struct CronSchedule {
    expression: String,
    minutes: Field,
    hours: Field,
    days_of_month: Field,
    months: Field,
    days_of_week: Field,
}

#[derive(Debug)]
struct Field {
    values: BTreeSet<u32>,
    any: bool,
}

impl CronSchedule {
    fn parse(expression: &str) -> Result<Self> {
        let parts: Vec<&str> = expression.split_whitespace().collect();
        if parts.len() != 5 {
            bail!(
                "Cron expression must have five fields: minute hour day-of-month month day-of-week"
            );
        }

        Ok(Self {
            expression: expression.to_string(),
            minutes: Field::parse(parts[0], 0, 59, FieldKind::Minute)?,
            hours: Field::parse(parts[1], 0, 23, FieldKind::Hour)?,
            days_of_month: Field::parse(parts[2], 1, 31, FieldKind::DayOfMonth)?,
            months: Field::parse(parts[3], 1, 12, FieldKind::Month)?,
            days_of_week: Field::parse(parts[4], 0, 7, FieldKind::DayOfWeek)?,
        })
    }

    fn last_between(
        &self,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Option<DateTime<Local>> {
        self.occurrences_between(start, end).last()
    }

    fn next_between(
        &self,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> Option<DateTime<Local>> {
        self.occurrences_between(start, end).next()
    }

    fn occurrences_between(
        &self,
        start: DateTime<Local>,
        end: DateTime<Local>,
    ) -> impl Iterator<Item = DateTime<Local>> + '_ {
        let mut current = round_down_to_minute(start);
        std::iter::from_fn(move || {
            while current <= end {
                let candidate = current;
                current += chrono::Duration::minutes(1);
                if candidate >= start && self.matches(candidate) {
                    return Some(candidate);
                }
            }
            None
        })
    }

    fn matches(&self, at: DateTime<Local>) -> bool {
        self.minutes.matches(at.minute())
            && self.hours.matches(at.hour())
            && self.months.matches(at.month())
            && self.matches_day(at)
    }

    fn matches_day(&self, at: DateTime<Local>) -> bool {
        let day_of_month_matches = self.days_of_month.matches(at.day());
        let weekday = at.weekday().num_days_from_sunday();
        let day_of_week_matches =
            self.days_of_week.matches(weekday) || (weekday == 0 && self.days_of_week.matches(7));

        match (self.days_of_month.any, self.days_of_week.any) {
            (true, true) => true,
            (true, false) => day_of_week_matches,
            (false, true) => day_of_month_matches,
            (false, false) => day_of_month_matches || day_of_week_matches,
        }
    }
}

impl Field {
    fn parse(raw: &str, min: u32, max: u32, kind: FieldKind) -> Result<Self> {
        let mut values = BTreeSet::new();
        let any = raw == "*";

        for part in raw.split(',') {
            if part.is_empty() {
                bail!("Empty cron field segment in {raw}");
            }
            add_field_segment(part, min, max, kind, &mut values)?;
        }

        Ok(Self { values, any })
    }

    fn matches(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

#[derive(Debug, Clone, Copy)]
enum FieldKind {
    Minute,
    Hour,
    DayOfMonth,
    Month,
    DayOfWeek,
}

fn add_field_segment(
    raw: &str,
    min: u32,
    max: u32,
    kind: FieldKind,
    values: &mut BTreeSet<u32>,
) -> Result<()> {
    let (range_raw, step) = if let Some((range, step_raw)) = raw.split_once('/') {
        let step = step_raw
            .parse::<u32>()
            .with_context(|| format!("Invalid cron step {step_raw}"))?;
        if step == 0 {
            bail!("Cron step cannot be zero");
        }
        (range, step)
    } else {
        (raw, 1)
    };

    let (start, end) = if range_raw == "*" {
        (min, max)
    } else if let Some((start_raw, end_raw)) = range_raw.split_once('-') {
        (
            parse_field_value(start_raw, kind)?,
            parse_field_value(end_raw, kind)?,
        )
    } else {
        let value = parse_field_value(range_raw, kind)?;
        (value, value)
    };

    if start < min || start > max || end < min || end > max || start > end {
        bail!("Cron field segment {raw} is outside allowed range {min}-{max}");
    }

    for value in (start..=end).step_by(step as usize) {
        values.insert(value);
    }
    Ok(())
}

fn parse_field_value(raw: &str, kind: FieldKind) -> Result<u32> {
    let lower = raw.to_ascii_lowercase();
    match kind {
        FieldKind::Month => month_name_value(&lower)
            .or_else(|| lower.parse::<u32>().ok())
            .with_context(|| format!("Invalid month value {raw}")),
        FieldKind::DayOfWeek => weekday_name_value(&lower)
            .or_else(|| lower.parse::<u32>().ok())
            .with_context(|| format!("Invalid day-of-week value {raw}")),
        _ => lower
            .parse::<u32>()
            .with_context(|| format!("Invalid cron value {raw}")),
    }
}

fn month_name_value(raw: &str) -> Option<u32> {
    match raw {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn weekday_name_value(raw: &str) -> Option<u32> {
    match raw {
        "sun" | "sunday" => Some(0),
        "mon" | "monday" => Some(1),
        "tue" | "tues" | "tuesday" => Some(2),
        "wed" | "wednesday" => Some(3),
        "thu" | "thur" | "thurs" | "thursday" => Some(4),
        "fri" | "friday" => Some(5),
        "sat" | "saturday" => Some(6),
        _ => None,
    }
}

fn round_down_to_minute(at: DateTime<Local>) -> DateTime<Local> {
    Local
        .with_ymd_and_hms(at.year(), at.month(), at.day(), at.hour(), at.minute(), 0)
        .single()
        .unwrap_or(at)
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
