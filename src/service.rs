use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::fs;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::cron::sync_cron_schedule;
use crate::icecast::{poll_icecast_stream, start_icecast_device_stream, stop_icecast_stream};
use crate::playback::{
    PlaybackStart, apply_live_audio_state, build_playbin_from_source, expand_playback_sources,
    is_remote_media_source, resolve_effective_audio, start_playbin_at_offset,
    validate_playback_source,
};
use crate::schedule::{
    load_schedule, remove_schedule_entry, sort_schedule_entries, validate_volume,
};
use crate::time_signal::{due_time_signal_tick, load_time_signal_config};
use crate::types::{
    FADE_TICK_MS, LiveOverrides, SERVICE_TICK_MS, ScheduleEntry, ServiceDirective, ServiceState,
};

pub fn run_service(db_path: &Path, config_path: &Path, socket_path: &Path) -> Result<()> {
    gst::init().context("Failed to initialize GStreamer")?;
    let listener = bind_service_socket(socket_path)?;
    let mut overrides = LiveOverrides::default();
    let mut state = ServiceState::new();
    let mut last_time_signal_tick: Option<i64> = None;
    let mut time_signal_overlays = Vec::new();
    let mut icecast_stream = match start_icecast_device_stream(config_path) {
        Ok(stream) => stream,
        Err(error) => {
            eprintln!("Failed to start Icecast device stream: {error:#}");
            None
        }
    };

    println!(
        "Service running. socket={} schedule_db={} config={}",
        socket_path.display(),
        db_path.display(),
        config_path.display()
    );

    loop {
        poll_icecast_stream(&mut icecast_stream);
        sync_cron_schedule(db_path)?;
        let mut db = load_schedule(db_path)?;
        sort_schedule_entries(&mut db.entries);

        let directive = process_pending_service_commands(
            &listener,
            &mut overrides,
            &mut state,
            db.entries.len(),
        )?;
        match directive {
            ServiceDirective::Continue
            | ServiceDirective::SkipCurrent
            | ServiceDirective::ReplaceCurrent => {}
            ServiceDirective::StopAudio => {
                stop_time_signal_overlays(&mut time_signal_overlays);
            }
            ServiceDirective::StopService => {
                println!("Service shutdown requested.");
                stop_time_signal_overlays(&mut time_signal_overlays);
                stop_icecast_stream(&mut icecast_stream);
                break;
            }
        }

        let now = chrono::Local::now();
        let next_due = db.entries.first().cloned().filter(|entry| entry.at <= now);
        if state.audio_enabled {
            maybe_start_time_signal_overlay(
                config_path,
                next_due.as_ref().map(|entry| entry.file.as_str()),
                &mut last_time_signal_tick,
                &mut time_signal_overlays,
            );
        }
        poll_time_signal_overlays(&mut time_signal_overlays);

        let Some(entry) = next_due.filter(|_| state.audio_enabled) else {
            state.now_playing = None;
            state.now_playing_id = None;
            thread::sleep(std::time::Duration::from_millis(SERVICE_TICK_MS));
            continue;
        };

        state.now_playing = Some(entry.file.clone());
        state.now_playing_id = Some(entry.id);

        let outcome = match play_entry_with_service_control(
            &entry,
            &listener,
            &mut overrides,
            &mut state,
            db_path,
            config_path,
            db.entries.len(),
            &mut last_time_signal_tick,
            &mut time_signal_overlays,
            &mut icecast_stream,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                eprintln!(
                    "Failed to play #{} {}: {error:#}. Removing failed schedule entry and continuing.",
                    entry.id, entry.file
                );
                remove_schedule_entry(db_path, entry.id).with_context(|| {
                    format!("Failed to remove failed schedule entry #{}", entry.id)
                })?;
                state.now_playing = None;
                state.now_playing_id = None;
                continue;
            }
        };

        match outcome {
            ServiceDirective::Continue
            | ServiceDirective::SkipCurrent
            | ServiceDirective::ReplaceCurrent => {
                remove_schedule_entry(db_path, entry.id)?;
                if outcome == ServiceDirective::ReplaceCurrent {
                    println!("Replaced and removed #{}", entry.id);
                } else {
                    println!("Completed and removed #{}", entry.id);
                }
            }
            ServiceDirective::StopService => {
                println!("Service shutdown requested during playback.");
                stop_time_signal_overlays(&mut time_signal_overlays);
                stop_icecast_stream(&mut icecast_stream);
                break;
            }
            ServiceDirective::StopAudio => {
                stop_time_signal_overlays(&mut time_signal_overlays);
                println!("Playback stopped for #{}", entry.id);
            }
        }

        state.now_playing = None;
        state.now_playing_id = None;
    }

    if socket_path.exists() {
        fs::remove_file(socket_path).with_context(|| {
            format!(
                "Failed to remove service socket after shutdown: {}",
                socket_path.display()
            )
        })?;
    }

    Ok(())
}

pub fn send_service_command(socket_path: &Path, command: &str) -> Result<String> {
    let mut stream = UnixStream::connect(socket_path).with_context(|| {
        format!(
            "Failed to connect to service socket {}. Is `radio-fm service run` active?",
            socket_path.display()
        )
    })?;
    stream
        .write_all(format!("{command}\n").as_bytes())
        .context("Failed to send command to service")?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .context("Failed to close service command write side")?;

    let mut response = String::new();
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .context("Failed to read response from service")?;
        if bytes == 0 {
            break;
        }
        response.push_str(&line);
    }
    if response.is_empty() {
        response = "error: empty response from service\n".to_string();
    }
    Ok(response)
}

fn bind_service_socket(socket_path: &Path) -> Result<UnixListener> {
    if socket_path.exists() {
        fs::remove_file(socket_path).with_context(|| {
            format!(
                "Failed to remove previous socket file {}",
                socket_path.display()
            )
        })?;
    }
    let listener = UnixListener::bind(socket_path).with_context(|| {
        format!(
            "Failed to bind service control socket at {}",
            socket_path.display()
        )
    })?;
    listener
        .set_nonblocking(true)
        .context("Failed to set service socket non-blocking mode")?;
    Ok(listener)
}

fn process_pending_service_commands(
    listener: &UnixListener,
    overrides: &mut LiveOverrides,
    state: &mut ServiceState,
    queued_items: usize,
) -> Result<ServiceDirective> {
    let mut aggregate = ServiceDirective::Continue;

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let directive = handle_service_stream(stream, overrides, state, queued_items)?;
                aggregate = merge_service_directive(aggregate, directive);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => return Err(error).context("Service socket accept failed"),
        }
    }

    Ok(aggregate)
}

fn merge_service_directive(
    aggregate: ServiceDirective,
    directive: ServiceDirective,
) -> ServiceDirective {
    match (aggregate, directive) {
        (_, ServiceDirective::StopService) | (ServiceDirective::StopService, _) => {
            ServiceDirective::StopService
        }
        (_, ServiceDirective::StopAudio) | (ServiceDirective::StopAudio, _) => {
            ServiceDirective::StopAudio
        }
        (_, ServiceDirective::ReplaceCurrent) | (ServiceDirective::ReplaceCurrent, _) => {
            ServiceDirective::ReplaceCurrent
        }
        (_, ServiceDirective::SkipCurrent) | (ServiceDirective::SkipCurrent, _) => {
            ServiceDirective::SkipCurrent
        }
        _ => ServiceDirective::Continue,
    }
}

fn handle_service_stream(
    mut stream: UnixStream,
    overrides: &mut LiveOverrides,
    state: &mut ServiceState,
    queued_items: usize,
) -> Result<ServiceDirective> {
    let mut command = String::new();
    let mut reader = BufReader::new(
        stream
            .try_clone()
            .context("Failed to clone service stream for reading")?,
    );
    reader
        .read_line(&mut command)
        .context("Failed reading service command")?;

    let (response, directive) =
        handle_service_command(command.trim(), overrides, state, queued_items)?;
    stream
        .write_all(response.as_bytes())
        .context("Failed writing service response")?;
    stream
        .flush()
        .context("Failed flushing service response stream")?;
    Ok(directive)
}

fn handle_service_command(
    command: &str,
    overrides: &mut LiveOverrides,
    state: &mut ServiceState,
    queued_items: usize,
) -> Result<(String, ServiceDirective)> {
    if command.is_empty() {
        return Ok((
            "error: empty command\n".to_string(),
            ServiceDirective::Continue,
        ));
    }

    let mut parts = command.split_whitespace();
    let Some(name) = parts.next() else {
        return Ok((
            "error: empty command\n".to_string(),
            ServiceDirective::Continue,
        ));
    };

    match name {
        "status" => {
            let now_playing = state.now_playing.as_deref().unwrap_or("none");
            let now_id = state
                .now_playing_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "none".to_string());
            let override_volume = overrides
                .volume
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "none".to_string());
            let override_mute = overrides
                .mute
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string());
            let audio = if state.audio_enabled {
                "enabled"
            } else {
                "stopped"
            };
            let response = format!(
                "ok: running audio={audio} now_playing_id={now_id} now_playing={now_playing} queued_items={queued_items} override_volume={override_volume} override_mute={override_mute}\n"
            );
            Ok((response, ServiceDirective::Continue))
        }
        "play" => {
            state.audio_enabled = true;
            Ok((
                "ok: audio playback enabled\n".to_string(),
                ServiceDirective::Continue,
            ))
        }
        "set-volume" => {
            let Some(value_text) = parts.next() else {
                return Ok((
                    "error: missing value. usage: set-volume <0.0..1.0>\n".to_string(),
                    ServiceDirective::Continue,
                ));
            };
            let value: f64 = value_text
                .parse()
                .with_context(|| format!("Invalid volume value: {value_text}"))?;
            validate_volume(value)?;
            overrides.volume = Some(value);
            Ok((
                format!("ok: override volume set to {value:.2}\n"),
                ServiceDirective::Continue,
            ))
        }
        "mute" => {
            let mode = parts.next().unwrap_or("on");
            let value = match mode {
                "on" | "true" | "1" => true,
                "off" | "false" | "0" => false,
                _ => {
                    return Ok((
                        "error: usage mute [on|off]\n".to_string(),
                        ServiceDirective::Continue,
                    ));
                }
            };
            overrides.mute = Some(value);
            Ok((
                format!("ok: override mute set to {value}\n"),
                ServiceDirective::Continue,
            ))
        }
        "skip" => Ok(("ok: skip requested\n".to_string(), ServiceDirective::SkipCurrent)),
        "stop" => {
            state.audio_enabled = false;
            Ok((
                "ok: audio playback stopped\n".to_string(),
                ServiceDirective::StopAudio,
            ))
        }
        "shutdown" => Ok((
            "ok: service shutdown requested\n".to_string(),
            ServiceDirective::StopService,
        )),
        "help" => Ok((
            "ok: commands: status | play | stop | set-volume <0.0..1.0> | mute [on|off] | skip | shutdown\n"
                .to_string(),
            ServiceDirective::Continue,
        )),
        _ => Ok((
            "error: unknown command. use: status | play | stop | set-volume <0.0..1.0> | mute [on|off] | skip | shutdown\n".to_string(),
            ServiceDirective::Continue,
        )),
    }
}

fn play_entry_with_service_control(
    entry: &ScheduleEntry,
    listener: &UnixListener,
    overrides: &mut LiveOverrides,
    state: &mut ServiceState,
    db_path: &Path,
    config_path: &Path,
    queued_items: usize,
    last_time_signal_tick: &mut Option<i64>,
    time_signal_overlays: &mut Vec<TimeSignalOverlay>,
    icecast_stream: &mut Option<crate::icecast::IcecastStream>,
) -> Result<ServiceDirective> {
    validate_volume(entry.volume)?;
    let start_offset = scheduled_start_offset(entry);
    let sources = expand_playback_sources(&entry.file)?;

    for (index, source) in sources.iter().enumerate() {
        state.now_playing = Some(source.clone());
        let outcome = play_source_with_service_control(
            entry,
            source,
            if index == 0 { entry.fade_in_secs } else { 0 },
            if index + 1 == sources.len() {
                entry.fade_out_secs
            } else {
                0
            },
            if index == 0 {
                start_offset
            } else {
                Duration::ZERO
            },
            listener,
            overrides,
            state,
            db_path,
            config_path,
            queued_items,
            last_time_signal_tick,
            time_signal_overlays,
            icecast_stream,
        )?;

        if outcome != ServiceDirective::Continue {
            return Ok(outcome);
        }
    }

    Ok(ServiceDirective::Continue)
}

fn play_source_with_service_control(
    entry: &ScheduleEntry,
    source: &str,
    fade_in_secs: u64,
    fade_out_secs: u64,
    start_offset: Duration,
    listener: &UnixListener,
    overrides: &mut LiveOverrides,
    state: &mut ServiceState,
    db_path: &Path,
    config_path: &Path,
    queued_items: usize,
    last_time_signal_tick: &mut Option<i64>,
    time_signal_overlays: &mut Vec<TimeSignalOverlay>,
    icecast_stream: &mut Option<crate::icecast::IcecastStream>,
) -> Result<ServiceDirective> {
    validate_playback_source(source)?;

    let playbin = build_playbin_from_source(source)?;
    apply_live_audio_state(&playbin, fade_in_secs, entry.volume, entry.mute, *overrides);

    if start_playbin_at_offset(&playbin, source, start_offset)? == PlaybackStart::PastEnd {
        println!(
            "Skipping #{} because offset {}s is past the end",
            entry.id,
            start_offset.as_secs()
        );
        return Ok(ServiceDirective::Continue);
    }
    let label = format!("#{}", entry.id);
    println!(
        "Playing {} {} (offset {}s, fade-in {}s, fade-out {}s, volume {:.2}, mute {})",
        label,
        source,
        start_offset.as_secs(),
        fade_in_secs,
        fade_out_secs,
        entry.volume,
        entry.mute
    );

    let bus = playbin.bus().context("Pipeline has no message bus")?;
    let fade_tick = gst::ClockTime::from_mseconds(FADE_TICK_MS);
    let fade_in_start = Instant::now()
        .checked_sub(start_offset)
        .unwrap_or_else(Instant::now);
    let mut fade_out_start: Option<Instant> = None;

    loop {
        poll_icecast_stream(icecast_stream);
        maybe_start_time_signal_overlay(
            config_path,
            Some(source),
            last_time_signal_tick,
            time_signal_overlays,
        );
        poll_time_signal_overlays(time_signal_overlays);

        let directive = process_pending_service_commands(listener, overrides, state, queued_items)?;
        match directive {
            ServiceDirective::Continue => {}
            ServiceDirective::SkipCurrent => {
                playbin
                    .set_state(gst::State::Null)
                    .context("Failed to stop pipeline on skip request")?;
                stop_time_signal_overlays(time_signal_overlays);
                println!("Skip requested for {}", label);
                return Ok(ServiceDirective::SkipCurrent);
            }
            ServiceDirective::StopAudio => {
                playbin
                    .set_state(gst::State::Null)
                    .context("Failed to stop pipeline on audio stop request")?;
                stop_time_signal_overlays(time_signal_overlays);
                println!("Stop requested for {}", label);
                return Ok(ServiceDirective::StopAudio);
            }
            ServiceDirective::StopService => {
                playbin
                    .set_state(gst::State::Null)
                    .context("Failed to stop pipeline on shutdown request")?;
                stop_time_signal_overlays(time_signal_overlays);
                return Ok(ServiceDirective::StopService);
            }
            ServiceDirective::ReplaceCurrent => {
                let (effective_volume, effective_mute) =
                    resolve_effective_audio(entry.volume, entry.mute, *overrides);
                fade_out_pipeline(
                    &playbin,
                    entry.fade_out_secs,
                    if effective_mute {
                        0.0
                    } else {
                        effective_volume
                    },
                );
                println!("Schedule replacement requested for {}", label);
                return Ok(ServiceDirective::ReplaceCurrent);
            }
        }

        if let Some(replacement) = pending_replacement(db_path, entry.id, entry.fade_out_secs)? {
            let (effective_volume, effective_mute) =
                resolve_effective_audio(entry.volume, entry.mute, *overrides);
            fade_out_pipeline(
                &playbin,
                replacement.fade_out_duration,
                if effective_mute {
                    0.0
                } else {
                    effective_volume
                },
            );
            println!("{} is replacing {}", replacement.label, label);
            return Ok(ServiceDirective::ReplaceCurrent);
        }

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

        let (effective_volume, effective_mute) =
            resolve_effective_audio(entry.volume, entry.mute, *overrides);
        let target_volume = if effective_mute {
            0.0
        } else {
            effective_volume
        };
        playbin.set_property("mute", effective_mute);

        if fade_out_start.is_none() && fade_in_secs > 0 && !effective_mute {
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
        } else if fade_in_secs == 0 || effective_mute {
            playbin.set_property("volume", target_volume);
        }
    }

    playbin
        .set_state(gst::State::Null)
        .context("Failed to stop GStreamer pipeline")?;
    Ok(ServiceDirective::Continue)
}

struct PendingReplacement {
    label: String,
    fade_out_duration: u64,
}

fn pending_replacement(
    db_path: &Path,
    current_id: u64,
    current_fade_out_secs: u64,
) -> Result<Option<PendingReplacement>> {
    let now = chrono::Local::now();
    sync_cron_schedule(db_path)?;
    let db = load_schedule(db_path)?;
    let fade_window = chrono::Duration::seconds(current_fade_out_secs as i64);
    Ok(db
        .entries
        .iter()
        .find(|entry| entry.id != current_id && entry.at <= now + fade_window)
        .map(|entry| {
            let remaining_secs = (entry.at - now)
                .to_std()
                .map(|duration| duration.as_secs())
                .unwrap_or(0);
            PendingReplacement {
                label: format!("Scheduled item #{}", entry.id),
                fade_out_duration: remaining_secs.min(current_fade_out_secs),
            }
        }))
}

struct TimeSignalOverlay {
    source: String,
    playbin: gst::Element,
}

fn maybe_start_time_signal_overlay(
    config_path: &Path,
    current_source: Option<&str>,
    last_tick: &mut Option<i64>,
    overlays: &mut Vec<TimeSignalOverlay>,
) {
    let now = chrono::Local::now();
    let config = match load_time_signal_config(config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Failed to load Greenwich time signal config: {error:#}");
            return;
        }
    };
    let Some(tick) = due_time_signal_tick(&config, now, *last_tick) else {
        return;
    };

    *last_tick = Some(tick);
    if !config.streams && current_source.is_some_and(is_remote_media_source) {
        if let Some(source) = current_source {
            println!(
                "Skipping Greenwich time signal for tick {tick} because stream is playing: {source}"
            );
        }
        return;
    }

    let Some(source) = config.source else {
        return;
    };

    let sources = match expand_playback_sources(&source) {
        Ok(sources) => sources,
        Err(error) => {
            eprintln!("Failed to expand Greenwich time signal {source}: {error:#}");
            return;
        }
    };

    for source in sources {
        match start_time_signal_overlay(&source) {
            Ok(overlay) => {
                println!("Playing Greenwich time signal overlay {source} for tick {tick}");
                overlays.push(overlay);
            }
            Err(error) => {
                eprintln!("Failed to play Greenwich time signal {source}: {error:#}. Continuing.");
            }
        }
    }
}

fn start_time_signal_overlay(source: &str) -> Result<TimeSignalOverlay> {
    validate_playback_source(source)?;
    let playbin = build_playbin_from_source(source)?;
    playbin.set_property("volume", 1.0f64);
    playbin.set_property("mute", false);
    playbin
        .set_state(gst::State::Playing)
        .context("Failed to set Greenwich time signal overlay to Playing")?;
    Ok(TimeSignalOverlay {
        source: source.to_string(),
        playbin,
    })
}

fn poll_time_signal_overlays(overlays: &mut Vec<TimeSignalOverlay>) {
    overlays.retain_mut(|overlay| {
        let Some(bus) = overlay.playbin.bus() else {
            let _ = overlay.playbin.set_state(gst::State::Null);
            eprintln!(
                "Stopping Greenwich time signal overlay {} because it has no message bus",
                overlay.source
            );
            return false;
        };

        let mut keep = true;
        while let Some(message) = bus.timed_pop(gst::ClockTime::ZERO) {
            use gst::MessageView;
            match message.view() {
                MessageView::Eos(..) => {
                    let _ = overlay.playbin.set_state(gst::State::Null);
                    println!("Completed Greenwich time signal overlay {}", overlay.source);
                    keep = false;
                    break;
                }
                MessageView::Error(err) => {
                    let src = err
                        .src()
                        .map(|s| s.path_string().to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let _ = overlay.playbin.set_state(gst::State::Null);
                    eprintln!(
                        "Greenwich time signal overlay error from {src} for {}: {}",
                        overlay.source,
                        err.error()
                    );
                    keep = false;
                    break;
                }
                _ => {}
            }
        }

        keep
    });
}

fn stop_time_signal_overlays(overlays: &mut Vec<TimeSignalOverlay>) {
    for overlay in overlays.drain(..) {
        let _ = overlay.playbin.set_state(gst::State::Null);
    }
}

fn scheduled_start_offset(entry: &ScheduleEntry) -> Duration {
    (chrono::Local::now() - entry.at)
        .to_std()
        .unwrap_or(Duration::ZERO)
}

fn fade_out_pipeline(playbin: &gst::Element, fade_out_secs: u64, start_volume: f64) {
    if fade_out_secs == 0 || start_volume <= 0.0 {
        let _ = playbin.set_state(gst::State::Null);
        return;
    }

    let started = Instant::now();
    loop {
        let ratio = (started.elapsed().as_secs_f64() / fade_out_secs as f64).min(1.0);
        playbin.set_property("volume", start_volume * (1.0 - ratio));
        if ratio >= 1.0 {
            break;
        }
        thread::sleep(Duration::from_millis(FADE_TICK_MS));
    }
    let _ = playbin.set_state(gst::State::Null);
}
