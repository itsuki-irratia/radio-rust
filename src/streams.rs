use anyhow::{Context, Result};
use std::path::Path;

use crate::config::load_app_config;
use crate::types::StreamDb;

pub fn run_streams_list(config_path: &Path, json: bool) -> Result<()> {
    let db = load_streams(config_path)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&db).context("Failed to serialize streams JSON")?
        );
        return Ok(());
    }

    if db.entries.is_empty() {
        println!("No streams in {}", config_path.display());
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

pub fn load_streams(config_path: &Path) -> Result<StreamDb> {
    let config = load_app_config(config_path)
        .with_context(|| format!("Failed to load streams from {}", config_path.display()))?;
    Ok(config.streams)
}
