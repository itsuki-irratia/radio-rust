use anyhow::{Context, Result, bail};
use chrono::{
    Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_EXPORT_MONTHS: usize = 120;

#[derive(Debug, Clone, Serialize)]
pub struct Program {
    pub slug: String,
    pub name: String,
    pub days: Vec<String>,
    pub start: String,
    pub end: String,
    pub description: Option<String>,
    pub platform: Option<String>,
    pub url: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgramException {
    pub slug: String,
    pub date: String,
    pub cancel: bool,
    pub start: Option<String>,
    pub end: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecialProgram {
    pub slug: String,
    pub name: String,
    pub date: String,
    pub start: String,
    pub end: String,
    pub description: Option<String>,
    pub platform: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProgramMetadata<'a> {
    pub description: Option<&'a str>,
    pub platform: Option<&'a str>,
    pub url: Option<&'a str>,
}

#[derive(Serialize)]
struct MonthExport {
    month: String,
    generated_at: String,
    timezone: &'static str,
    events: Vec<ExportEvent>,
}

#[derive(Serialize)]
struct ExportEvent {
    id: String,
    slug: String,
    name: String,
    starts_at: String,
    ends_at: String,
    origin: &'static str,
    kind: &'static str,
    cancelled: bool,
    description: Option<String>,
    platform: Option<String>,
    url: Option<String>,
}

#[derive(Clone)]
struct Occurrence {
    id: String,
    slug: String,
    name: String,
    date: NaiveDate,
    start_minute: u16,
    end_minute: u16,
    kind: &'static str,
    cancelled: bool,
    description: Option<String>,
    platform: Option<String>,
    url: Option<String>,
}

pub fn run_program_add(
    db_path: &Path,
    slug: &str,
    name: &str,
    days: &str,
    start: &str,
    end: &str,
    metadata: ProgramMetadata<'_>,
) -> Result<()> {
    validate_slug_name(slug, name)?;
    let days_mask = parse_days(days)?;
    let (start_minute, end_minute) = parse_time_range(start, end)?;
    validate_metadata(metadata)?;
    let conn = open_programs_db(db_path)?;
    conn.execute(
        "INSERT INTO programs
            (slug, name, days_mask, start_minute, end_minute, description, platform, url, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
        params![
            slug,
            name.trim(),
            days_mask,
            start_minute,
            end_minute,
            clean_optional(metadata.description),
            clean_optional(metadata.platform),
            clean_optional(metadata.url),
        ],
    )
    .with_context(|| format!("Failed to add program {slug}"))?;
    println!(
        "Added program {slug} | {} | {} | {}-{}",
        name.trim(),
        format_days(days_mask),
        format_minute(start_minute),
        format_minute(end_minute)
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn run_program_update(
    db_path: &Path,
    slug: &str,
    name: Option<&str>,
    days: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    description: Option<&str>,
    platform: Option<&str>,
    url: Option<&str>,
    enabled: Option<bool>,
) -> Result<()> {
    if let Some(name) = name {
        if name.trim().is_empty() {
            bail!("Program name cannot be empty");
        }
    }
    let conn = open_programs_db(db_path)?;
    let current =
        load_program(&conn, slug)?.with_context(|| format!("Program not found: {slug}"))?;
    let current_days = parse_days(&current.days.join(","))?;
    let days_mask = days.map(parse_days).transpose()?.unwrap_or(current_days);
    let start_minute = start
        .map(parse_time)
        .transpose()?
        .unwrap_or(parse_time(&current.start)?);
    let end_minute = end
        .map(parse_time)
        .transpose()?
        .unwrap_or(parse_time(&current.end)?);
    validate_time_minutes(start_minute, end_minute)?;
    let description = merge_optional_update(description, current.description.as_deref());
    let platform = merge_optional_update(platform, current.platform.as_deref());
    let url = merge_optional_update(url, current.url.as_deref());
    validate_metadata(ProgramMetadata {
        description: description.as_deref(),
        platform: platform.as_deref(),
        url: url.as_deref(),
    })?;

    let changed = conn.execute(
        "UPDATE programs SET
            name = ?1, days_mask = ?2, start_minute = ?3, end_minute = ?4,
            description = ?5, platform = ?6, url = ?7, enabled = ?8
         WHERE slug = ?9",
        params![
            name.unwrap_or(&current.name).trim(),
            days_mask,
            start_minute,
            end_minute,
            description,
            platform,
            url,
            enabled.unwrap_or(current.enabled),
            slug,
        ],
    )?;
    if changed == 0 {
        bail!("Program not found: {slug}");
    }
    println!("Updated program {slug}");
    Ok(())
}

pub fn run_program_list(db_path: &Path, json: bool) -> Result<()> {
    let conn = open_programs_db(db_path)?;
    let programs = load_programs(&conn)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&programs)?);
    } else if programs.is_empty() {
        println!("No programs in {}", db_path.display());
    } else {
        for program in programs {
            println!(
                "{} | {} | {} | {}-{} | enabled {}",
                program.slug,
                program.name,
                program.days.join(","),
                program.start,
                program.end,
                program.enabled
            );
        }
    }
    Ok(())
}

pub fn run_program_remove(db_path: &Path, slug: &str) -> Result<()> {
    let conn = open_programs_db(db_path)?;
    let removed = conn.execute("DELETE FROM programs WHERE slug = ?1", [slug])?;
    if removed == 0 {
        bail!("Program not found: {slug}");
    }
    println!("Removed program {slug} and its exceptions");
    Ok(())
}

pub fn run_exception_set(
    db_path: &Path,
    slug: &str,
    date: &str,
    cancel: bool,
    start: Option<&str>,
    end: Option<&str>,
    name: Option<&str>,
) -> Result<()> {
    let date = parse_date(date)?;
    let (start_minute, end_minute) = if cancel {
        if start.is_some() || end.is_some() || name.is_some() {
            bail!("A cancellation cannot also override the time or name");
        }
        (None, None)
    } else {
        match (start, end) {
            (Some(start), Some(end)) => {
                let (start, end) = parse_time_range(start, end)?;
                (Some(start), Some(end))
            }
            (None, None) if name.is_some() => (None, None),
            _ => bail!("Set an exception with --cancel, both --start/--end, or --name"),
        }
    };
    if name.is_some_and(|value| value.trim().is_empty()) {
        bail!("Exception name cannot be empty");
    }

    let conn = open_programs_db(db_path)?;
    let (program_id, days_mask): (i64, i64) = conn
        .query_row(
            "SELECT id, days_mask FROM programs WHERE slug = ?1",
            [slug],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .with_context(|| format!("Program not found: {slug}"))?;
    if (days_mask as u8) & weekday_bit(date.weekday()) == 0 {
        bail!("{slug} is not scheduled on {date}; add a special program instead");
    }
    conn.execute(
        "INSERT INTO program_exceptions
            (program_id, date, cancelled, start_minute, end_minute, name_override)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(program_id, date) DO UPDATE SET
            cancelled = excluded.cancelled,
            start_minute = excluded.start_minute,
            end_minute = excluded.end_minute,
            name_override = excluded.name_override",
        params![
            program_id,
            date.to_string(),
            i64::from(cancel),
            start_minute,
            end_minute,
            name.map(str::trim),
        ],
    )?;
    println!("Set exception for {slug} on {date}");
    Ok(())
}

pub fn run_exception_list(
    db_path: &Path,
    from: Option<&str>,
    to: Option<&str>,
    json: bool,
) -> Result<()> {
    let range = parse_date_range(from, to)?;
    let conn = open_programs_db(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT p.slug, e.date, e.cancelled, e.start_minute, e.end_minute, e.name_override
         FROM program_exceptions e JOIN programs p ON p.id = e.program_id
         ORDER BY e.date, p.slug",
    )?;
    let exceptions = stmt
        .query_map([], |row| {
            let start: Option<i64> = row.get(3)?;
            let end: Option<i64> = row.get(4)?;
            Ok(ProgramException {
                slug: row.get(0)?,
                date: row.get(1)?,
                cancel: row.get::<_, i64>(2)? != 0,
                start: start.map(|value| format_minute(value as u16)),
                end: end.map(|value| format_minute(value as u16)),
                name: row.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|exception| {
            let date = NaiveDate::parse_from_str(&exception.date, "%Y-%m-%d").unwrap_or_default();
            range.is_none_or(|(from, to)| from <= date && date <= to)
        })
        .collect::<Vec<_>>();
    if json {
        println!("{}", serde_json::to_string_pretty(&exceptions)?);
    } else if exceptions.is_empty() {
        println!("No program exceptions in {}", db_path.display());
    } else {
        for exception in exceptions {
            let detail = if exception.cancel {
                "cancelled".to_string()
            } else if let (Some(start), Some(end)) =
                (exception.start.as_deref(), exception.end.as_deref())
            {
                format!("{start}-{end}")
            } else {
                "name override".to_string()
            };
            println!(
                "{} | {} | {}{}",
                exception.slug,
                exception.date,
                detail,
                exception
                    .name
                    .map(|name| format!(" | {name}"))
                    .unwrap_or_default()
            );
        }
    }
    Ok(())
}

pub fn run_exception_remove(db_path: &Path, slug: &str, date: &str) -> Result<()> {
    let date = parse_date(date)?;
    let conn = open_programs_db(db_path)?;
    let removed = conn.execute(
        "DELETE FROM program_exceptions WHERE date = ?1 AND program_id =
            (SELECT id FROM programs WHERE slug = ?2)",
        params![date.to_string(), slug],
    )?;
    if removed == 0 {
        bail!("No exception for {slug} on {date}");
    }
    println!("Removed exception for {slug} on {date}");
    Ok(())
}

pub fn run_special_add(
    db_path: &Path,
    slug: &str,
    name: &str,
    date: &str,
    start: &str,
    end: &str,
    metadata: ProgramMetadata<'_>,
) -> Result<()> {
    validate_slug_name(slug, name)?;
    let date = parse_date(date)?;
    let (start, end) = parse_time_range(start, end)?;
    validate_metadata(metadata)?;
    let conn = open_programs_db(db_path)?;
    conn.execute(
        "INSERT INTO special_programs
            (slug, name, date, start_minute, end_minute, description, platform, url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            slug,
            name.trim(),
            date.to_string(),
            start,
            end,
            clean_optional(metadata.description),
            clean_optional(metadata.platform),
            clean_optional(metadata.url),
        ],
    )
    .with_context(|| format!("Failed to add special program {slug} on {date}"))?;
    println!("Added special program {slug} on {date}");
    Ok(())
}

pub fn run_special_list(
    db_path: &Path,
    from: Option<&str>,
    to: Option<&str>,
    json: bool,
) -> Result<()> {
    let range = parse_date_range(from, to)?;
    let conn = open_programs_db(db_path)?;
    let mut stmt = conn.prepare(
        "SELECT slug, name, date, start_minute, end_minute, description, platform, url
         FROM special_programs ORDER BY date, start_minute, slug",
    )?;
    let specials = stmt
        .query_map([], |row| {
            let start: i64 = row.get(3)?;
            let end: i64 = row.get(4)?;
            Ok(SpecialProgram {
                slug: row.get(0)?,
                name: row.get(1)?,
                date: row.get(2)?,
                start: format_minute(start as u16),
                end: format_minute(end as u16),
                description: row.get(5)?,
                platform: row.get(6)?,
                url: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|special| {
            let date = NaiveDate::parse_from_str(&special.date, "%Y-%m-%d").unwrap_or_default();
            range.is_none_or(|(from, to)| from <= date && date <= to)
        })
        .collect::<Vec<_>>();
    if json {
        println!("{}", serde_json::to_string_pretty(&specials)?);
    } else if specials.is_empty() {
        println!("No special programs in {}", db_path.display());
    } else {
        for special in specials {
            println!(
                "{} | {}-{} | {} | {}",
                special.date, special.start, special.end, special.name, special.slug
            );
        }
    }
    Ok(())
}

pub fn run_special_remove(db_path: &Path, slug: &str, date: &str) -> Result<()> {
    let date = parse_date(date)?;
    let conn = open_programs_db(db_path)?;
    let removed = conn.execute(
        "DELETE FROM special_programs WHERE slug = ?1 AND date = ?2",
        params![slug, date.to_string()],
    )?;
    if removed == 0 {
        bail!("Special program {slug} not found on {date}");
    }
    println!("Removed special program {slug} on {date}");
    Ok(())
}

pub fn run_program_export(
    db_path: &Path,
    from_month: &str,
    to_month: &str,
    output_dir: &Path,
) -> Result<()> {
    let from = parse_month(from_month)?;
    let to = parse_month(to_month)?;
    if from > to {
        bail!("Invalid month range: --from-month is after --to-month");
    }
    let month_count = month_distance(from, to) + 1;
    if month_count > MAX_EXPORT_MONTHS {
        bail!("Export range exceeds the {MAX_EXPORT_MONTHS} month limit");
    }

    let conn = open_programs_db(db_path)?;
    let programs = load_programs_raw(&conn)?;
    let mut exports = Vec::new();
    let mut month = from;
    loop {
        let month_start = first_of_month(month.0, month.1)?;
        let next_month = next_month(month.0, month.1)?;
        let month_end = next_month.pred_opt().context("Invalid month end")?;
        let events = expand_month(&conn, &programs, month_start, month_end)?;
        let generated_at = Local::now().to_rfc3339();
        let month_name = format!("{:04}-{:02}", month.0, month.1);
        let payload = MonthExport {
            month: month_name.clone(),
            generated_at,
            timezone: "local",
            events,
        };
        let raw = format!("{}\n", serde_json::to_string_pretty(&payload)?);
        let path = output_dir.join(format!("{month_name}.json"));
        write_export_atomically(&path, raw.as_bytes())?;
        exports.push(path);
        if month == to {
            break;
        }
        month = increment_month(month)?;
    }
    for path in exports {
        println!("Exported {}", path.display());
    }
    Ok(())
}

fn open_programs_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create programs database directory {}",
                parent.display()
            )
        })?;
    }
    let conn = Connection::open(path)
        .with_context(|| format!("Failed to open programs database {}", path.display()))?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS programs (
             id INTEGER PRIMARY KEY,
             slug TEXT NOT NULL UNIQUE,
             name TEXT NOT NULL,
             days_mask INTEGER NOT NULL CHECK (days_mask BETWEEN 1 AND 127),
             start_minute INTEGER NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
             end_minute INTEGER NOT NULL CHECK (end_minute BETWEEN 1 AND 1440),
             description TEXT,
             platform TEXT,
             url TEXT,
             enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
             CHECK (start_minute < end_minute)
         );
         CREATE TABLE IF NOT EXISTS program_exceptions (
             id INTEGER PRIMARY KEY,
             program_id INTEGER NOT NULL REFERENCES programs(id) ON DELETE CASCADE,
             date TEXT NOT NULL,
             cancelled INTEGER NOT NULL DEFAULT 0 CHECK (cancelled IN (0, 1)),
             start_minute INTEGER,
             end_minute INTEGER,
             name_override TEXT,
             UNIQUE (program_id, date),
             CHECK ((start_minute IS NULL AND end_minute IS NULL)
                 OR (start_minute >= 0 AND start_minute < end_minute AND end_minute <= 1440))
         );
         CREATE TABLE IF NOT EXISTS special_programs (
             id INTEGER PRIMARY KEY,
             slug TEXT NOT NULL,
             name TEXT NOT NULL,
             date TEXT NOT NULL,
             start_minute INTEGER NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
             end_minute INTEGER NOT NULL CHECK (end_minute BETWEEN 1 AND 1440),
             description TEXT,
             platform TEXT,
             url TEXT,
             UNIQUE (slug, date),
             CHECK (start_minute < end_minute)
         );
         CREATE INDEX IF NOT EXISTS program_exceptions_date_idx ON program_exceptions(date);
         CREATE INDEX IF NOT EXISTS special_programs_date_idx ON special_programs(date);",
    )
    .context("Failed to initialize programs database schema")?;
    Ok(conn)
}

fn load_programs(conn: &Connection) -> Result<Vec<Program>> {
    load_programs_raw(conn)
}

fn load_programs_raw(conn: &Connection) -> Result<Vec<Program>> {
    let mut stmt = conn.prepare(
        "SELECT slug, name, days_mask, start_minute, end_minute, description, platform, url, enabled
         FROM programs ORDER BY slug",
    )?;
    let rows = stmt.query_map([], |row| {
        let days_mask: i64 = row.get(2)?;
        let start: i64 = row.get(3)?;
        let end: i64 = row.get(4)?;
        Ok(Program {
            slug: row.get(0)?,
            name: row.get(1)?,
            days: day_names(days_mask as u8),
            start: format_minute(start as u16),
            end: format_minute(end as u16),
            description: row.get(5)?,
            platform: row.get(6)?,
            url: row.get(7)?,
            enabled: row.get::<_, i64>(8)? != 0,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("Failed to read programs")
}

fn load_program(conn: &Connection, slug: &str) -> Result<Option<Program>> {
    let mut stmt = conn.prepare(
        "SELECT slug, name, days_mask, start_minute, end_minute, description, platform, url, enabled
         FROM programs WHERE slug = ?1",
    )?;
    stmt.query_row([slug], |row| {
        let days_mask: i64 = row.get(2)?;
        let start: i64 = row.get(3)?;
        let end: i64 = row.get(4)?;
        Ok(Program {
            slug: row.get(0)?,
            name: row.get(1)?,
            days: day_names(days_mask as u8),
            start: format_minute(start as u16),
            end: format_minute(end as u16),
            description: row.get(5)?,
            platform: row.get(6)?,
            url: row.get(7)?,
            enabled: row.get::<_, i64>(8)? != 0,
        })
    })
    .optional()
    .context("Failed to load program")
}

fn expand_month(
    conn: &Connection,
    programs: &[Program],
    first: NaiveDate,
    last: NaiveDate,
) -> Result<Vec<ExportEvent>> {
    let mut exceptions = std::collections::HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT p.slug, e.date, e.cancelled, e.start_minute, e.end_minute, e.name_override
         FROM program_exceptions e JOIN programs p ON p.id = e.program_id
         WHERE e.date >= ?1 AND e.date <= ?2",
    )?;
    let exception_rows = stmt.query_map(params![first.to_string(), last.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? != 0,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;
    for exception in exception_rows {
        let (slug, date, cancel, start, end, name) = exception?;
        exceptions.insert((slug, date), (cancel, start, end, name));
    }

    let mut occurrences = Vec::new();
    let mut date = first;
    loop {
        for program in programs.iter().filter(|program| program.enabled) {
            let day_mask = parse_days(&program.days.join(","))?;
            if (day_mask as u8) & weekday_bit(date.weekday()) == 0 {
                continue;
            }
            let key = (program.slug.clone(), date.to_string());
            let exception = exceptions.remove(&key);
            let was_exception = exception.is_some();
            if exception.as_ref().is_some_and(|(cancel, _, _, _)| *cancel) {
                occurrences.push(Occurrence {
                    id: format!("program/{}/{date}", program.slug),
                    slug: program.slug.clone(),
                    name: program.name.clone(),
                    date,
                    start_minute: parse_time(&program.start)?,
                    end_minute: parse_time(&program.end)?,
                    kind: "exception",
                    cancelled: true,
                    description: program.description.clone(),
                    platform: program.platform.clone(),
                    url: program.url.clone(),
                });
                continue;
            }
            let (start, end, name) = match exception {
                Some((_, Some(start), Some(end), name)) => (start as u16, end as u16, name),
                Some((_, None, None, name)) => {
                    (parse_time(&program.start)?, parse_time(&program.end)?, name)
                }
                Some(_) => bail!("Corrupt time override for {} on {date}", program.slug),
                None => (parse_time(&program.start)?, parse_time(&program.end)?, None),
            };
            occurrences.push(Occurrence {
                id: format!("program/{}/{date}", program.slug),
                slug: program.slug.clone(),
                name: name.unwrap_or_else(|| program.name.clone()),
                date,
                start_minute: start,
                end_minute: end,
                kind: if was_exception {
                    "exception"
                } else {
                    "recurring"
                },
                cancelled: false,
                description: program.description.clone(),
                platform: program.platform.clone(),
                url: program.url.clone(),
            });
        }
        if date == last {
            break;
        }
        date = date.succ_opt().context("Date is out of range")?;
    }

    let mut special_stmt = conn.prepare(
        "SELECT slug, name, date, start_minute, end_minute, description, platform, url
         FROM special_programs WHERE date >= ?1 AND date <= ?2",
    )?;
    let special_rows =
        special_stmt.query_map(params![first.to_string(), last.to_string()], |row| {
            Ok(Occurrence {
                id: format!(
                    "special/{}/{}",
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(2)?
                ),
                slug: row.get(0)?,
                name: row.get(1)?,
                date: NaiveDate::parse_from_str(&row.get::<_, String>(2)?, "%Y-%m-%d").map_err(
                    |error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    },
                )?,
                start_minute: row.get::<_, i64>(3)? as u16,
                end_minute: row.get::<_, i64>(4)? as u16,
                kind: "special",
                cancelled: false,
                description: row.get(5)?,
                platform: row.get(6)?,
                url: row.get(7)?,
            })
        })?;
    occurrences.extend(special_rows.collect::<rusqlite::Result<Vec<_>>>()?);

    occurrences.sort_by(|left, right| {
        left.date
            .cmp(&right.date)
            .then(left.start_minute.cmp(&right.start_minute))
            .then(left.slug.cmp(&right.slug))
    });
    occurrences
        .into_iter()
        .map(|occurrence| {
            let starts_at = local_datetime(occurrence.date, occurrence.start_minute)?;
            let ends_at = local_datetime(occurrence.date, occurrence.end_minute)?;
            Ok(ExportEvent {
                id: occurrence.id,
                slug: occurrence.slug,
                name: occurrence.name,
                starts_at: starts_at.to_rfc3339(),
                ends_at: ends_at.to_rfc3339(),
                origin: "external_live",
                kind: occurrence.kind,
                cancelled: occurrence.cancelled,
                description: occurrence.description,
                platform: occurrence.platform,
                url: occurrence.url,
            })
        })
        .collect()
}

fn local_datetime(date: NaiveDate, minute: u16) -> Result<chrono::DateTime<Local>> {
    let time = if minute == 1440 {
        NaiveTime::MIN
    } else {
        NaiveTime::from_hms_opt(u32::from(minute / 60), u32::from(minute % 60), 0)
            .context("Invalid program time")?
    };
    let date = if minute == 1440 {
        date.succ_opt()
            .context("Program end date is out of range")?
    } else {
        date
    };
    let naive = NaiveDateTime::new(date, time);
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(first, _) => Ok(first),
        LocalResult::None => bail!(
            "Local time {} on {} does not exist because of a timezone transition",
            time,
            date
        ),
    }
}

fn parse_days(raw: &str) -> Result<i64> {
    let mut mask = 0u8;
    for segment in raw.split(',') {
        if segment.trim().is_empty() {
            bail!("Empty day in --days. Use values like mon,tue,wed or mon-thu");
        }
        if let Some((start, end)) = segment.trim().split_once('-') {
            let start = parse_weekday(start)?;
            let end = parse_weekday(end)?;
            let mut index = start;
            loop {
                mask |= 1 << index;
                if index == end {
                    break;
                }
                index = (index + 1) % 7;
            }
        } else {
            mask |= 1 << parse_weekday(segment.trim())?;
        }
    }
    Ok(i64::from(mask))
}

fn parse_weekday(raw: &str) -> Result<u8> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "mon" | "monday" => Ok(0),
        "tue" | "tues" | "tuesday" => Ok(1),
        "wed" | "wednesday" => Ok(2),
        "thu" | "thur" | "thurs" | "thursday" => Ok(3),
        "fri" | "friday" => Ok(4),
        "sat" | "saturday" => Ok(5),
        "sun" | "sunday" => Ok(6),
        _ => bail!("Invalid weekday {raw}. Use mon,tue,wed,thu,fri,sat,sun"),
    }
}

fn weekday_bit(day: Weekday) -> u8 {
    1 << day.num_days_from_monday()
}

fn day_names(mask: u8) -> Vec<String> {
    ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .iter()
        .enumerate()
        .filter(|(index, _)| mask & (1 << index) != 0)
        .map(|(_, name)| (*name).to_string())
        .collect()
}

fn format_days(mask: i64) -> String {
    day_names(mask as u8).join(",")
}

fn parse_time_range(start: &str, end: &str) -> Result<(u16, u16)> {
    let start = parse_time(start)?;
    let end = parse_time(end)?;
    validate_time_minutes(start, end)?;
    Ok((start, end))
}

fn validate_time_minutes(start: u16, end: u16) -> Result<()> {
    if start >= end {
        bail!("Program end time must be later than its start time on the same day");
    }
    Ok(())
}

fn parse_time(raw: &str) -> Result<u16> {
    if raw == "24:00" {
        return Ok(1440);
    }
    let time = NaiveTime::parse_from_str(raw, "%H:%M")
        .with_context(|| format!("Invalid time {raw}. Use HH:MM, from 00:00 to 23:59"))?;
    Ok((time.hour() * 60 + time.minute()) as u16)
}

fn format_minute(value: u16) -> String {
    if value == 1440 {
        return "24:00".to_string();
    }
    format!("{:02}:{:02}", value / 60, value % 60)
}

fn validate_slug_name(slug: &str, name: &str) -> Result<()> {
    if slug.trim().is_empty()
        || slug
            .chars()
            .any(|char| !(char.is_ascii_alphanumeric() || "-_".contains(char)))
    {
        bail!("Slug must contain only ASCII letters, numbers, hyphens, or underscores");
    }
    if name.trim().is_empty() {
        bail!("Program name cannot be empty");
    }
    Ok(())
}

fn validate_metadata(metadata: ProgramMetadata<'_>) -> Result<()> {
    for (label, value) in [
        ("description", metadata.description),
        ("platform", metadata.platform),
        ("URL", metadata.url),
    ] {
        if value.is_some_and(|value| value.trim().is_empty()) {
            bail!("Program {label} cannot be empty");
        }
    }
    if metadata
        .url
        .is_some_and(|url| !url.starts_with("https://") && !url.starts_with("http://"))
    {
        bail!("Program URL must start with http:// or https://");
    }
    Ok(())
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value.map(str::trim).map(str::to_string)
}

fn merge_optional_update(value: Option<&str>, current: Option<&str>) -> Option<String> {
    match value {
        Some(value) if value.trim().is_empty() => None,
        Some(value) => Some(value.trim().to_string()),
        None => current.map(str::to_string),
    }
}

fn parse_date(raw: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d")
        .with_context(|| format!("Invalid date {raw}. Use YYYY-MM-DD"))
}

fn parse_date_range(
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Option<(NaiveDate, NaiveDate)>> {
    match (from, to) {
        (None, None) => Ok(None),
        (Some(from), Some(to)) => {
            let from = parse_date(from)?;
            let to = parse_date(to)?;
            if from > to {
                bail!("Invalid date range: --from is after --to");
            }
            Ok(Some((from, to)))
        }
        _ => bail!("Use both --from and --to"),
    }
}

fn parse_month(raw: &str) -> Result<(i32, u32)> {
    let date = NaiveDate::parse_from_str(&format!("{raw}-01"), "%Y-%m-%d")
        .with_context(|| format!("Invalid month {raw}. Use YYYY-MM"))?;
    Ok((date.year(), date.month()))
}

fn first_of_month(year: i32, month: u32) -> Result<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, 1).context("Invalid calendar month")
}

fn next_month(year: i32, month: u32) -> Result<NaiveDate> {
    if month == 12 {
        first_of_month(
            year.checked_add(1)
                .context("Calendar year is out of range")?,
            1,
        )
    } else {
        first_of_month(year, month + 1)
    }
}

fn increment_month((year, month): (i32, u32)) -> Result<(i32, u32)> {
    if month == 12 {
        Ok((
            year.checked_add(1)
                .context("Calendar year is out of range")?,
            1,
        ))
    } else {
        Ok((year, month + 1))
    }
}

fn month_distance(from: (i32, u32), to: (i32, u32)) -> usize {
    ((to.0 - from.0) * 12 + to.1 as i32 - from.1 as i32) as usize
}

fn write_export_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create export directory {}", parent.display()))?;
    let filename = path
        .file_name()
        .context("Export path has no filename")?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{filename}.{}.{}.tmp", std::process::id(), nonce));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&temporary)
            .with_context(|| {
                format!("Failed to create temporary export {}", temporary.display())
            })?;
        file.write_all(contents)
            .with_context(|| format!("Failed to write temporary export {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("Failed to sync temporary export {}", temporary.display()))?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o644)).with_context(|| {
            format!(
                "Failed to set public export permissions {}",
                temporary.display()
            )
        })?;
        fs::rename(&temporary, path)
            .with_context(|| format!("Failed to replace export {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::path::PathBuf;

    #[test]
    fn parses_weekday_ranges_and_combinations() {
        assert_eq!(parse_days("mon-thu,fri").unwrap(), 31);
        assert_eq!(parse_days("sat,sun").unwrap(), 96);
    }

    #[test]
    fn program_update_can_clear_optional_metadata() {
        let path = test_db_path();
        run_program_add(
            &path,
            "gmv",
            "GOOD MORNING VIETNAM",
            "mon-thu",
            "09:09",
            "11:00",
            ProgramMetadata {
                description: Some("Live show"),
                platform: Some("External"),
                url: Some("https://example.org/live"),
            },
        )
        .unwrap();
        run_program_update(
            &path,
            "gmv",
            None,
            None,
            None,
            None,
            Some(""),
            Some(""),
            Some(""),
            None,
        )
        .unwrap();

        let program = load_program(&open_programs_db(&path).unwrap(), "gmv")
            .unwrap()
            .unwrap();
        assert_eq!(program.description, None);
        assert_eq!(program.platform, None);
        assert_eq!(program.url, None);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn monthly_export_expands_weekly_program_and_applies_exception_and_special() {
        let path = test_db_path();
        let conn = open_programs_db(&path).unwrap();
        conn.execute(
            "INSERT INTO programs (slug, name, days_mask, start_minute, end_minute, enabled)
             VALUES ('gmv', 'GOOD MORNING VIETNAM', 15, 549, 660, 1)",
            [],
        )
        .unwrap();
        let program_id: i64 = conn
            .query_row("SELECT id FROM programs WHERE slug = 'gmv'", [], |row| {
                row.get(0)
            })
            .unwrap();
        conn.execute(
            "INSERT INTO program_exceptions (program_id, date, start_minute, end_minute)
             VALUES (?1, '2026-10-05', 600, 660)",
            [program_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO program_exceptions (program_id, date, cancelled)
             VALUES (?1, '2026-10-06', 1)",
            [program_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO special_programs (slug, name, date, start_minute, end_minute)
             VALUES ('special', 'Special', '2026-10-10', 600, 660)",
            [],
        )
        .unwrap();

        let programs = load_programs(&conn).unwrap();
        let events = expand_month(
            &conn,
            &programs,
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 10, 31).unwrap(),
        )
        .unwrap();

        assert_eq!(events.len(), 18);
        assert!(
            events
                .iter()
                .any(|event| { event.id == "program/gmv/2026-10-05" && event.kind == "exception" })
        );
        assert!(
            events
                .iter()
                .any(|event| { event.id == "program/gmv/2026-10-06" && event.cancelled })
        );
        assert!(events.iter().any(|event| event.kind == "special"));
        drop(conn);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn export_writes_one_full_month_file_for_every_inclusive_month() {
        let db_path = test_db_path();
        let output_dir = db_path.with_extension("export");

        run_program_export(&db_path, "2026-10", "2026-12", &output_dir).unwrap();

        for month in ["2026-10", "2026-11", "2026-12"] {
            let path = output_dir.join(format!("{month}.json"));
            let json: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert_eq!(json["month"], month);
            assert_eq!(json["timezone"], "local");
            assert!(json["events"].as_array().unwrap().is_empty());
        }

        let _ = fs::remove_file(db_path);
        let _ = fs::remove_dir_all(output_dir);
    }

    fn test_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "radio-fm-programs-test-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
