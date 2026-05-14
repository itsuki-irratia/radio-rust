use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

use crate::types::{
    AppConfig, DEFAULT_CONFIG_DIR_NAME, DEFAULT_CONFIG_FILE_NAME, DEFAULT_SCHEDULE_DB_FILE_NAME,
};

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
    let mut needs_save = !config_path.exists();
    let config = if config_path.exists() {
        let raw = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file {}", config_path.display()))?;
        if raw.trim().is_empty() {
            needs_save = true;
            AppConfig::default()
        } else {
            if !raw.contains("\"icecast\"") {
                needs_save = true;
            }
            if !raw.contains("\"fade\"")
                || raw.contains("\"default_fade_in_secs\"")
                || raw.contains("\"default_fade_out_secs\"")
            {
                needs_save = true;
            }
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse config file {}", config_path.display()))?
        }
    } else {
        AppConfig::default()
    };

    if needs_save {
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

fn validate_app_config(config: &AppConfig) -> Result<()> {
    if config.fade.duration > i64::MAX as u64 {
        bail!(
            "Invalid fade duration {}. Use a smaller value",
            config.fade.duration
        );
    }
    let volume = config.playback.default_volume;
    if !(0.0..=1.0).contains(&volume) {
        bail!("Invalid default volume {volume}. Use a value between 0.0 and 1.0");
    }
    Ok(())
}
