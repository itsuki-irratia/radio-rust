use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::config::{load_app_config, update_app_config};
use crate::playback::is_remote_media_source;
use crate::types::{StreamDb, StreamEntry};

pub fn run_streams_add(config_path: &Path, slug: &str, name: &str, url: &str) -> Result<()> {
    validate_stream_input(slug, name, url)?;

    let mut added = false;
    let mut updated = false;
    let mut saved_stream = None;
    update_app_config(config_path, |config| {
        let next_id = config
            .streams
            .entries
            .iter()
            .map(|entry| entry.id)
            .max()
            .unwrap_or(0)
            + 1;

        let slug_index = config
            .streams
            .entries
            .iter()
            .position(|entry| entry.slug == slug);
        let url_index = config
            .streams
            .entries
            .iter()
            .position(|entry| entry.url == url);
        if slug_index.is_some() && url_index.is_some() && slug_index != url_index {
            bail!("Stream slug and URL belong to different existing entries");
        }

        if let Some(index) = slug_index.or(url_index) {
            let entry = &mut config.streams.entries[index];
            updated = entry.slug != slug || entry.name != name || entry.url != url;
            entry.slug = slug.to_string();
            entry.name = name.to_string();
            entry.url = url.to_string();
            saved_stream = Some(entry.clone());
            return Ok(());
        }

        added = true;
        let entry = StreamEntry {
            id: next_id,
            slug: slug.to_string(),
            name: name.to_string(),
            url: url.to_string(),
        };
        config.streams.entries.push(entry.clone());
        saved_stream = Some(entry);
        Ok(())
    })?;

    let stream = saved_stream.context("Failed to save stream entry")?;
    if added {
        println!(
            "Added stream #{} | {} | {} | {}",
            stream.id, stream.slug, stream.name, stream.url
        );
    } else if updated {
        println!(
            "Updated stream #{} | {} | {} | {}",
            stream.id, stream.slug, stream.name, stream.url
        );
    } else {
        println!(
            "Stream already exists #{} | {} | {} | {}",
            stream.id, stream.slug, stream.name, stream.url
        );
    }

    Ok(())
}

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

fn validate_stream_input(slug: &str, name: &str, url: &str) -> Result<()> {
    if slug.trim().is_empty() {
        bail!("Stream slug cannot be empty");
    }
    if slug.chars().any(char::is_whitespace) {
        bail!("Stream slug cannot contain whitespace");
    }
    if name.trim().is_empty() {
        bail!("Stream name cannot be empty");
    }
    if !is_remote_media_source(url) {
        bail!("Stream URL must start with http:// or https://");
    }
    Ok(())
}
