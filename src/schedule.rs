use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, LocalResult, NaiveDateTime, NaiveTime, TimeZone};
use gstreamer as gst;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use crate::playback::{canonical_playback_source, play_file_with_fades};
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
    validate_volume(volume)?;

    let at_dt = parse_scheduled_datetime(at)?;
    let mut db = load_schedule(db_path)?;

    let next_id = db.entries.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
    db.entries.push(ScheduleEntry {
        id: next_id,
        file: canonical_source.clone(),
        at: at_dt,
        fade_in_secs: fade_in,
        fade_out_secs: fade_out,
        volume,
        mute,
    });
    sort_schedule_entries(&mut db.entries);
    save_schedule(db_path, &db)?;

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

pub fn run_schedule_list(db_path: &Path, json: bool) -> Result<()> {
    let mut db = load_schedule(db_path)?;
    sort_schedule_entries(&mut db.entries);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&db).context("Failed to serialize schedule JSON")?
        );
        return Ok(());
    }

    if db.entries.is_empty() {
        println!("No scheduled items in {}", db_path.display());
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

        let file = PathBuf::from(&next.file);
        play_file_with_fades(
            &file,
            next.fade_in_secs,
            next.fade_out_secs,
            next.volume,
            next.mute,
        )?;

        db.entries.retain(|entry| entry.id != next.id);
        save_schedule(db_path, &db)?;
        println!("Completed and removed #{}", next.id);
    }
}

pub fn load_schedule(db_path: &Path) -> Result<ScheduleDb> {
    if !db_path.exists() {
        return Ok(ScheduleDb::default());
    }

    let raw = fs::read_to_string(db_path)
        .with_context(|| format!("Failed to read schedule file {}", db_path.display()))?;
    let mut db: ScheduleDb = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse schedule file {}", db_path.display()))?;
    sort_schedule_entries(&mut db.entries);
    Ok(db)
}

pub fn save_schedule(db_path: &Path, db: &ScheduleDb) -> Result<()> {
    let raw = serde_json::to_string_pretty(db).context("Failed to serialize schedule file")?;
    fs::write(db_path, raw)
        .with_context(|| format!("Failed to write schedule file {}", db_path.display()))?;
    Ok(())
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
