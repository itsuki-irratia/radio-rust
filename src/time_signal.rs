use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, Timelike};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

use crate::playback::{canonical_playback_source, expand_playback_sources};
use crate::schedule::open_schedule_db;
use crate::types::TimeSignalConfig;

const TIME_SIGNAL_CONFIG_ID: i64 = 1;
const SECS_PER_MINUTE: i64 = 60;

pub fn run_time_signal_set_audio(db_path: &Path, source: &str) -> Result<()> {
    let canonical_source = canonical_playback_source(source)?;
    expand_playback_sources(&canonical_source)?;

    let conn = open_time_signal_db(db_path)?;
    ensure_time_signal_row(&conn)?;
    conn.execute(
        "UPDATE time_signal_config
         SET source = ?1, updated_at_rfc3339 = ?2
         WHERE id = ?3",
        params![
            &canonical_source,
            Local::now().to_rfc3339(),
            TIME_SIGNAL_CONFIG_ID
        ],
    )
    .context("Failed to set Greenwich time signal audio")?;

    println!("Greenwich time signal audio set to {canonical_source}");
    Ok(())
}

pub fn run_time_signal_enable(db_path: &Path) -> Result<()> {
    let conn = open_time_signal_db(db_path)?;
    ensure_time_signal_row(&conn)?;
    let config = load_time_signal_config_from_connection(&conn)?;
    if config.source.is_none() {
        bail!("Set Greenwich time signal audio before enabling it");
    }

    set_time_signal_enabled(&conn, true)?;
    println!("Greenwich time signal enabled");
    Ok(())
}

pub fn run_time_signal_disable(db_path: &Path) -> Result<()> {
    let conn = open_time_signal_db(db_path)?;
    ensure_time_signal_row(&conn)?;
    set_time_signal_enabled(&conn, false)?;
    println!("Greenwich time signal disabled");
    Ok(())
}

pub fn run_time_signal_disable_during_streams(db_path: &Path) -> Result<()> {
    let conn = open_time_signal_db(db_path)?;
    ensure_time_signal_row(&conn)?;
    set_time_signal_skip_during_streams(&conn, true)?;
    println!("Greenwich time signal disabled while streams are playing");
    Ok(())
}

pub fn run_time_signal_enable_during_streams(db_path: &Path) -> Result<()> {
    let conn = open_time_signal_db(db_path)?;
    ensure_time_signal_row(&conn)?;
    set_time_signal_skip_during_streams(&conn, false)?;
    println!("Greenwich time signal enabled while streams are playing");
    Ok(())
}

pub fn run_time_signal_status(db_path: &Path, json: bool) -> Result<()> {
    let config = load_time_signal_config(db_path)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&config)
                .context("Failed to serialize time signal JSON")?
        );
        return Ok(());
    }

    let source = config.source.as_deref().unwrap_or("none");
    println!(
        "Greenwich time signal | enabled {} | audio {} | disabled-during-streams {}",
        config.enabled, source, config.skip_during_streams
    );
    Ok(())
}

pub fn load_time_signal_config(db_path: &Path) -> Result<TimeSignalConfig> {
    let conn = open_time_signal_db(db_path)?;
    ensure_time_signal_row(&conn)?;
    load_time_signal_config_from_connection(&conn)
}

pub fn due_time_signal_tick(
    config: &TimeSignalConfig,
    now: DateTime<Local>,
    last_tick: Option<i64>,
) -> Option<i64> {
    if !config.enabled || config.source.is_none() || now.second() != 0 {
        return None;
    }

    let tick = minute_key(now);
    (Some(tick) != last_tick).then_some(tick)
}

fn open_time_signal_db(db_path: &Path) -> Result<Connection> {
    let conn = open_schedule_db(db_path)?;
    init_time_signal_schema(&conn)?;
    Ok(conn)
}

fn init_time_signal_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS time_signal_config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            source TEXT,
            enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
            skip_during_streams INTEGER NOT NULL DEFAULT 0 CHECK (skip_during_streams IN (0, 1)),
            updated_at_rfc3339 TEXT NOT NULL
        );
        ",
    )
    .context("Failed to initialize Greenwich time signal schema")?;
    add_time_signal_column_if_missing(
        conn,
        "skip_during_streams",
        "INTEGER NOT NULL DEFAULT 0 CHECK (skip_during_streams IN (0, 1))",
    )?;
    Ok(())
}

fn ensure_time_signal_row(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO time_signal_config
            (id, source, enabled, skip_during_streams, updated_at_rfc3339)
         VALUES (?1, NULL, 0, 0, ?2)",
        params![TIME_SIGNAL_CONFIG_ID, Local::now().to_rfc3339()],
    )
    .context("Failed to initialize Greenwich time signal config")?;
    Ok(())
}

fn load_time_signal_config_from_connection(conn: &Connection) -> Result<TimeSignalConfig> {
    let config = conn
        .query_row(
            "SELECT enabled, source, skip_during_streams
             FROM time_signal_config
             WHERE id = ?1",
            params![TIME_SIGNAL_CONFIG_ID],
            |row| {
                let enabled: i64 = row.get(0)?;
                let skip_during_streams: i64 = row.get(2)?;
                Ok(TimeSignalConfig {
                    enabled: enabled != 0,
                    source: row.get(1)?,
                    skip_during_streams: skip_during_streams != 0,
                })
            },
        )
        .optional()
        .context("Failed to load Greenwich time signal config")?;

    Ok(config.unwrap_or(TimeSignalConfig {
        enabled: false,
        source: None,
        skip_during_streams: false,
    }))
}

fn set_time_signal_enabled(conn: &Connection, enabled: bool) -> Result<()> {
    conn.execute(
        "UPDATE time_signal_config
         SET enabled = ?1, updated_at_rfc3339 = ?2
         WHERE id = ?3",
        params![
            if enabled { 1 } else { 0 },
            Local::now().to_rfc3339(),
            TIME_SIGNAL_CONFIG_ID
        ],
    )
    .context("Failed to update Greenwich time signal enabled flag")?;
    Ok(())
}

fn set_time_signal_skip_during_streams(conn: &Connection, skip: bool) -> Result<()> {
    conn.execute(
        "UPDATE time_signal_config
         SET skip_during_streams = ?1, updated_at_rfc3339 = ?2
         WHERE id = ?3",
        params![
            if skip { 1 } else { 0 },
            Local::now().to_rfc3339(),
            TIME_SIGNAL_CONFIG_ID
        ],
    )
    .context("Failed to update Greenwich time signal stream behavior")?;
    Ok(())
}

fn add_time_signal_column_if_missing(conn: &Connection, name: &str, data_type: &str) -> Result<()> {
    let sql = format!("ALTER TABLE time_signal_config ADD COLUMN {name} {data_type}");
    match conn.execute(&sql, []) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(error) => {
            Err(error).with_context(|| format!("Failed to add Greenwich time signal column {name}"))
        }
    }
}

fn minute_key(now: DateTime<Local>) -> i64 {
    now.timestamp().div_euclid(SECS_PER_MINUTE)
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::due_time_signal_tick;
    use crate::types::TimeSignalConfig;

    fn enabled_config() -> TimeSignalConfig {
        TimeSignalConfig {
            enabled: true,
            source: Some("/tmp/pips.mp3".to_string()),
            skip_during_streams: false,
        }
    }

    #[test]
    fn due_only_at_start_of_minute() {
        let due = Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 0).unwrap();
        let later = Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 1).unwrap();

        assert!(due_time_signal_tick(&enabled_config(), due, None).is_some());
        assert_eq!(due_time_signal_tick(&enabled_config(), later, None), None);
    }

    #[test]
    fn due_tick_does_not_repeat_same_minute() {
        let due = Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 0).unwrap();
        let last_tick = due_time_signal_tick(&enabled_config(), due, None);

        assert_eq!(
            due_time_signal_tick(&enabled_config(), due, last_tick),
            None
        );
    }
}
