use anyhow::{Context, Result};
use rusqlite::types::Type;
use rusqlite::{Connection, Row, params};
use std::path::Path;

use crate::schedule::open_schedule_db;
use crate::types::{StreamDb, StreamEntry};

const BUILTIN_STREAMS: &[(&str, &str, &str)] = &[(
    "bizkaia-irratia",
    "Bizkaia Irratia",
    "https://server12.mediasector.es/listen/bizkaia_irratia/bizkaiairratia.mp3",
)];

pub fn run_streams_list(db_path: &Path, json: bool) -> Result<()> {
    let db = load_streams(db_path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&db).context("Failed to serialize streams JSON")?
        );
        return Ok(());
    }

    if db.entries.is_empty() {
        println!("No streams in {}", db_path.display());
        return Ok(());
    }

    for entry in db.entries {
        println!(
            "#{} | {} | {} | {}",
            entry.id, entry.slug, entry.name, entry.url
        );
    }
    Ok(())
}

pub fn load_streams(db_path: &Path) -> Result<StreamDb> {
    let conn = open_streams_db(db_path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, slug, name, url
             FROM streams
             ORDER BY name COLLATE NOCASE ASC, id ASC",
        )
        .context("Failed to prepare streams query")?;
    let entries = stmt
        .query_map([], stream_entry_from_row)
        .context("Failed to query streams")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Failed to read streams")?;
    Ok(StreamDb { entries })
}

fn open_streams_db(db_path: &Path) -> Result<Connection> {
    let conn = open_schedule_db(db_path)?;
    init_streams_schema(&conn)?;
    seed_builtin_streams(&conn)?;
    Ok(conn)
}

fn init_streams_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS streams (
            id INTEGER PRIMARY KEY,
            slug TEXT NOT NULL UNIQUE,
            name TEXT NOT NULL,
            url TEXT NOT NULL UNIQUE
        );
        CREATE INDEX IF NOT EXISTS streams_name_idx
            ON streams (name COLLATE NOCASE, id);
        ",
    )
    .context("Failed to initialize streams database schema")
}

fn seed_builtin_streams(conn: &Connection) -> Result<()> {
    for (slug, name, url) in BUILTIN_STREAMS {
        conn.execute(
            "INSERT OR IGNORE INTO streams (slug, name, url)
             VALUES (?1, ?2, ?3)",
            params![slug, name, url],
        )
        .with_context(|| format!("Failed to seed stream {slug}"))?;
    }
    Ok(())
}

fn stream_entry_from_row(row: &Row<'_>) -> rusqlite::Result<StreamEntry> {
    let id: i64 = row.get(0)?;

    Ok(StreamEntry {
        id: u64::try_from(id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, Type::Integer, Box::new(error))
        })?,
        slug: row.get(1)?,
        name: row.get(2)?,
        url: row.get(3)?,
    })
}
