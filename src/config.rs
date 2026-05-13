use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{
    AppConfig, DEFAULT_CONFIG_DIR_NAME, DEFAULT_CONFIG_FILE_NAME, DEFAULT_SCHEDULE_DB_FILE_NAME,
    StreamEntry,
};

const BUILTIN_STREAMS: &[(&str, &str, &str)] = &[(
    "bizkaia-irratia",
    "Bizkaia Irratia",
    "https://server12.mediasector.es/listen/bizkaia_irratia/bizkaiairratia.mp3",
)];

pub fn default_config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join(DEFAULT_CONFIG_DIR_NAME))
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join(DEFAULT_CONFIG_FILE_NAME))
}

pub fn default_schedule_db_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join(DEFAULT_SCHEDULE_DB_FILE_NAME))
}

pub fn resolve_config_path(config_path: Option<PathBuf>) -> Result<PathBuf> {
    match config_path {
        Some(path) => Ok(path),
        None => default_config_path(),
    }
}

pub fn resolve_db_path(db_path: Option<PathBuf>) -> Result<PathBuf> {
    match db_path {
        Some(path) => Ok(path),
        None => default_schedule_db_path(),
    }
}

pub fn load_app_config(config_path: &Path) -> Result<AppConfig> {
    let mut config = if config_path.exists() {
        let raw = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file {}", config_path.display()))?;
        if raw.trim().is_empty() {
            AppConfig::default()
        } else {
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse config file {}", config_path.display()))?
        }
    } else {
        AppConfig::default()
    };

    if ensure_builtin_streams(&mut config) || !config_path.exists() {
        save_app_config(config_path, &config)?;
    }

    Ok(config)
}

pub fn save_app_config(config_path: &Path, config: &AppConfig) -> Result<()> {
    validate_app_config(config)?;
    if let Some(parent) = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(config).context("Failed to serialize app config")?;
    fs::write(config_path, format!("{raw}\n"))
        .with_context(|| format!("Failed to write config file {}", config_path.display()))?;
    Ok(())
}

pub fn update_app_config(
    config_path: &Path,
    update: impl FnOnce(&mut AppConfig) -> Result<()>,
) -> Result<AppConfig> {
    let mut config = load_app_config(config_path)?;
    update(&mut config)?;
    save_app_config(config_path, &config)?;
    Ok(config)
}

fn ensure_builtin_streams(config: &mut AppConfig) -> bool {
    let mut changed = false;
    let mut next_id = config
        .streams
        .entries
        .iter()
        .map(|entry| entry.id)
        .max()
        .unwrap_or(0)
        + 1;

    for (slug, name, url) in BUILTIN_STREAMS {
        if config
            .streams
            .entries
            .iter()
            .any(|entry| entry.slug == *slug || entry.url == *url)
        {
            continue;
        }
        config.streams.entries.push(StreamEntry {
            id: next_id,
            slug: (*slug).to_string(),
            name: (*name).to_string(),
            url: (*url).to_string(),
        });
        next_id += 1;
        changed = true;
    }

    config.streams.entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });
    changed
}

fn validate_app_config(config: &AppConfig) -> Result<()> {
    let volume = config.playback.default_volume;
    if !(0.0..=1.0).contains(&volume) {
        bail!("Invalid default volume {volume}. Use a value between 0.0 and 1.0");
    }
    Ok(())
}
