use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::schedule::validate_volume;
use crate::types::{FADE_TICK_MS, LiveOverrides};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStart {
    Started,
    PastEnd,
}

pub fn play_file_with_fades_from(
    file: &Path,
    fade_in_secs: u64,
    fade_out_secs: u64,
    volume: f64,
    mute: bool,
    start_offset: Duration,
) -> Result<()> {
    validate_playback_source(&file.display().to_string())?;
    validate_volume(volume)?;

    gst::init().context("Failed to initialize GStreamer")?;
    let playbin = build_playbin(file)?;
    let target_volume = if mute { 0.0 } else { volume };
    playbin.set_property("mute", mute);

    if fade_in_secs > 0 && !mute {
        playbin.set_property("volume", 0.0f64);
    } else {
        playbin.set_property("volume", target_volume);
    }

    if start_playbin_at_offset(&playbin, &file.display().to_string(), start_offset)?
        == PlaybackStart::PastEnd
    {
        println!(
            "Skipping {} because offset {}s is past the end",
            file.display(),
            start_offset.as_secs()
        );
        return Ok(());
    }

    println!(
        "Playing {} (offset {}s, fade-in {}s, fade-out {}s, volume {:.2}, mute {})",
        file.display(),
        start_offset.as_secs(),
        fade_in_secs,
        fade_out_secs,
        volume,
        mute
    );

    let bus = playbin.bus().context("Pipeline has no message bus")?;
    let fade_tick = gst::ClockTime::from_mseconds(FADE_TICK_MS);
    let fade_in_start = Instant::now()
        .checked_sub(start_offset)
        .unwrap_or_else(Instant::now);
    let mut fade_out_start: Option<Instant> = None;

    loop {
        if let Some(message) = bus.timed_pop(fade_tick) {
            use gst::MessageView;
            match message.view() {
                MessageView::Eos(..) => break,
                MessageView::Error(err) => {
                    let src = err
                        .src()
                        .map(|s| s.path_string().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    playbin
                        .set_state(gst::State::Null)
                        .context("Failed to stop GStreamer pipeline after playback error")?;
                    bail!("Playback error from {src}: {}", err.error());
                }
                _ => {}
            }
        }

        if fade_out_start.is_none() && fade_in_secs > 0 && !mute {
            let ratio = (fade_in_start.elapsed().as_secs_f64() / fade_in_secs as f64).min(1.0);
            playbin.set_property("volume", target_volume * ratio);
        }

        if fade_out_secs > 0 && fade_out_start.is_none() {
            if let (Some(duration), Some(position)) = (
                playbin.query_duration::<gst::ClockTime>(),
                playbin.query_position::<gst::ClockTime>(),
            ) {
                let duration_ns = duration.nseconds();
                let position_ns = position.nseconds();
                if duration_ns > position_ns {
                    let remaining_secs = (duration_ns - position_ns) as f64 / 1_000_000_000.0;
                    if remaining_secs <= fade_out_secs as f64 {
                        fade_out_start = Some(Instant::now());
                    }
                }
            }
        }

        if let Some(started) = fade_out_start {
            let ratio = (started.elapsed().as_secs_f64() / fade_out_secs as f64).min(1.0);
            playbin.set_property("volume", target_volume * (1.0 - ratio));
        }
    }

    playbin
        .set_state(gst::State::Null)
        .context("Failed to stop GStreamer pipeline")?;
    Ok(())
}

pub fn resolve_effective_audio(
    base_volume: f64,
    base_mute: bool,
    overrides: LiveOverrides,
) -> (f64, bool) {
    let volume = overrides.volume.unwrap_or(base_volume);
    let mute = overrides.mute.unwrap_or(base_mute);
    (volume, mute)
}

pub fn apply_live_audio_state(
    playbin: &gst::Element,
    fade_in_secs: u64,
    base_volume: f64,
    base_mute: bool,
    overrides: LiveOverrides,
) {
    let (effective_volume, effective_mute) =
        resolve_effective_audio(base_volume, base_mute, overrides);
    playbin.set_property("mute", effective_mute);
    if fade_in_secs > 0 && !effective_mute {
        playbin.set_property("volume", 0.0f64);
    } else if effective_mute {
        playbin.set_property("volume", 0.0f64);
    } else {
        playbin.set_property("volume", effective_volume);
    }
}

pub fn build_playbin(file: &Path) -> Result<gst::Element> {
    let playbin = gst::ElementFactory::make("playbin")
        .build()
        .context("Could not create GStreamer playbin element")?;

    let source = file.display().to_string();
    let uri = source_to_uri(&source)?;
    playbin.set_property("uri", uri.as_str());

    Ok(playbin)
}

pub fn canonical_playback_source(source: &str) -> Result<String> {
    if is_remote_media_source(source) {
        return Ok(source.to_string());
    }

    let path = Path::new(source);
    if !path.exists() {
        bail!("File does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("Path is not a file: {}", path.display());
    }

    Ok(path
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize file {}", path.display()))?
        .display()
        .to_string())
}

pub fn validate_playback_source(source: &str) -> Result<()> {
    if is_remote_media_source(source) {
        return Ok(());
    }

    let path = Path::new(source);
    if !path.exists() {
        bail!("File does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("Path is not a file: {}", path.display());
    }
    Ok(())
}

fn source_to_uri(source: &str) -> Result<String> {
    if is_remote_media_source(source) {
        return Ok(source.to_string());
    }

    let absolute = Path::new(source)
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize file {source}"))?;
    gst::glib::filename_to_uri(absolute, None)
        .map(|uri| uri.to_string())
        .context("Failed to convert file path into URI")
}

pub fn is_remote_media_source(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

pub fn seek_to_start_offset(
    playbin: &gst::Element,
    source: &str,
    start_offset: Duration,
) -> Result<()> {
    if start_offset.is_zero() || is_remote_media_source(source) {
        return Ok(());
    }

    let position =
        gst::ClockTime::from_nseconds(start_offset.as_nanos().min(u64::MAX as u128) as u64);
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        if playbin
            .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, position)
            .is_ok()
        {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    playbin
        .seek_simple(gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT, position)
        .with_context(|| format!("Failed to seek {source} to {}s", start_offset.as_secs()))?;
    Ok(())
}

pub fn start_playbin_at_offset(
    playbin: &gst::Element,
    source: &str,
    start_offset: Duration,
) -> Result<PlaybackStart> {
    if start_offset.is_zero() || is_remote_media_source(source) {
        playbin
            .set_state(gst::State::Playing)
            .context("Failed to set playback state to Playing")?;
        return Ok(PlaybackStart::Started);
    }

    playbin
        .set_state(gst::State::Paused)
        .context("Failed to set playback state to Paused before seeking")?;
    let (state_result, _, _) = playbin.state(gst::ClockTime::from_seconds(5));
    state_result.context("Failed to preroll playback before seeking")?;

    let offset_ns = start_offset.as_nanos().min(u64::MAX as u128) as u64;
    if let Some(duration) = playbin.query_duration::<gst::ClockTime>() {
        if offset_ns >= duration.nseconds() {
            let _ = playbin.set_state(gst::State::Null);
            return Ok(PlaybackStart::PastEnd);
        }
    }

    seek_to_start_offset(playbin, source, start_offset)?;

    playbin
        .set_state(gst::State::Playing)
        .context("Failed to set playback state to Playing after seeking")?;
    Ok(PlaybackStart::Started)
}
