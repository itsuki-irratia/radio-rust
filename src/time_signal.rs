use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local, Timelike};
use std::path::Path;

use crate::config::{load_app_config, update_app_config};
use crate::playback::{canonical_playback_source, expand_playback_sources};
use crate::types::TimeSignalConfig;

const SECS_PER_HOUR: i64 = 60 * 60;

pub fn run_time_signal_set_audio(config_path: &Path, source: &str) -> Result<()> {
    let canonical_source = canonical_playback_source(source)?;
    expand_playback_sources(&canonical_source)?;

    update_app_config(config_path, |config| {
        config.time_signal.source = Some(canonical_source.clone());
        Ok(())
    })
    .context("Failed to set Greenwich time signal audio")?;

    println!("Greenwich time signal audio set to {canonical_source}");
    Ok(())
}

pub fn run_time_signal_enable(config_path: &Path) -> Result<()> {
    let config = load_time_signal_config(config_path)?;
    if config.source.is_none() {
        bail!("Set Greenwich time signal audio before enabling it");
    }

    update_app_config(config_path, |config| {
        config.time_signal.enabled = true;
        Ok(())
    })
    .context("Failed to update Greenwich time signal enabled flag")?;
    println!("Greenwich time signal enabled");
    Ok(())
}

pub fn run_time_signal_disable(config_path: &Path) -> Result<()> {
    update_app_config(config_path, |config| {
        config.time_signal.enabled = false;
        Ok(())
    })
    .context("Failed to update Greenwich time signal enabled flag")?;
    println!("Greenwich time signal disabled");
    Ok(())
}

pub fn run_time_signal_disable_during_streams(config_path: &Path) -> Result<()> {
    run_time_signal_set_streams(config_path, false)?;
    println!("Greenwich time signal disabled while streams are playing");
    Ok(())
}

pub fn run_time_signal_enable_during_streams(config_path: &Path) -> Result<()> {
    run_time_signal_set_streams(config_path, true)?;
    println!("Greenwich time signal enabled while streams are playing");
    Ok(())
}

pub fn run_time_signal_set_streams(config_path: &Path, streams: bool) -> Result<()> {
    update_app_config(config_path, |config| {
        config.time_signal.streams = streams;
        Ok(())
    })
    .context("Failed to update Greenwich time signal stream behavior")?;
    Ok(())
}

pub fn run_time_signal_status(config_path: &Path, json: bool) -> Result<()> {
    let config = load_time_signal_config(config_path)?;
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
        "Greenwich time signal | enabled {} | audio {} | streams {}",
        config.enabled, source, config.streams
    );
    Ok(())
}

pub fn load_time_signal_config(config_path: &Path) -> Result<TimeSignalConfig> {
    let config = load_app_config(config_path)
        .with_context(|| format!("Failed to load config file {}", config_path.display()))?;
    Ok(config.time_signal)
}

pub fn due_time_signal_tick(
    config: &TimeSignalConfig,
    now: DateTime<Local>,
    last_tick: Option<i64>,
) -> Option<i64> {
    if !config.enabled || config.source.is_none() || now.minute() != 0 || now.second() != 0 {
        return None;
    }

    let tick = hour_key(now);
    (Some(tick) != last_tick).then_some(tick)
}

fn hour_key(now: DateTime<Local>) -> i64 {
    now.timestamp().div_euclid(SECS_PER_HOUR)
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
            streams: true,
        }
    }

    #[test]
    fn due_only_on_the_hour() {
        let due = Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 0).unwrap();
        let later_same_minute = Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 1).unwrap();
        let next_minute = Local.with_ymd_and_hms(2026, 5, 11, 18, 1, 0).unwrap();

        assert!(due_time_signal_tick(&enabled_config(), due, None).is_some());
        assert_eq!(
            due_time_signal_tick(&enabled_config(), later_same_minute, None),
            None
        );
        assert_eq!(
            due_time_signal_tick(&enabled_config(), next_minute, None),
            None
        );
    }

    #[test]
    fn due_tick_does_not_repeat_same_hour() {
        let due = Local.with_ymd_and_hms(2026, 5, 11, 18, 0, 0).unwrap();
        let last_tick = due_time_signal_tick(&enabled_config(), due, None);

        assert_eq!(
            due_time_signal_tick(&enabled_config(), due, last_tick),
            None
        );
    }
}
