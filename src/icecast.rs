use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use serde::Serialize;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::config::{load_app_config, update_app_config};
use crate::playback::is_remote_media_source;
use crate::types::IcecastConfig;

const DEFAULT_ICECAST_PORT: u16 = 8000;
const CONNECT_TIMEOUT_SECS: u64 = 5;

pub struct IcecastConfigure {
    pub server: String,
    pub mount: String,
    pub username: String,
    pub password: String,
    pub device: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub public: bool,
    pub enabled: bool,
}

#[derive(Serialize)]
struct IcecastStatus<'a> {
    enabled: bool,
    server: Option<&'a str>,
    device: Option<&'a str>,
    mount: &'a str,
    username: &'a str,
    password_set: bool,
    name: Option<&'a str>,
    description: Option<&'a str>,
    genre: Option<&'a str>,
    public: bool,
}

pub fn run_icecast_configure(config_path: &Path, input: IcecastConfigure) -> Result<()> {
    let server = normalize_server(&input.server)?;
    validate_mount(&input.mount)?;
    if let Some(device) = input.device.as_deref() {
        validate_device(device)?;
    }

    update_app_config(config_path, |config| {
        config.icecast = IcecastConfig {
            enabled: input.enabled,
            server: Some(server.clone()),
            device: input
                .device
                .clone()
                .or_else(|| config.icecast.device.clone()),
            mount: input.mount.clone(),
            username: input.username.clone(),
            password: Some(input.password.clone()),
            name: input.name.clone(),
            description: input.description.clone(),
            genre: input.genre.clone(),
            public: input.public,
        };
        Ok(())
    })
    .context("Failed to update Icecast config")?;

    println!(
        "Icecast configured: server={} mount={} enabled={}",
        server, input.mount, input.enabled
    );
    Ok(())
}

pub fn run_icecast_enable(config_path: &Path) -> Result<()> {
    ensure_icecast_ready(config_path)?;
    update_app_config(config_path, |config| {
        config.icecast.enabled = true;
        Ok(())
    })
    .context("Failed to enable Icecast")?;
    println!("Icecast enabled");
    Ok(())
}

pub fn run_icecast_disable(config_path: &Path) -> Result<()> {
    update_app_config(config_path, |config| {
        config.icecast.enabled = false;
        Ok(())
    })
    .context("Failed to disable Icecast")?;
    println!("Icecast disabled");
    Ok(())
}

pub fn run_icecast_status(config_path: &Path, json: bool) -> Result<()> {
    let config = load_app_config(config_path)?.icecast;
    let status = IcecastStatus {
        enabled: config.enabled,
        server: config.server.as_deref(),
        device: config.device.as_deref(),
        mount: &config.mount,
        username: &config.username,
        password_set: config.password.is_some(),
        name: config.name.as_deref(),
        description: config.description.as_deref(),
        genre: config.genre.as_deref(),
        public: config.public,
    };

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&status)
                .context("Failed to serialize Icecast status JSON")?
        );
        return Ok(());
    }

    println!(
        "Icecast | enabled {} | server {} | device {} | mount {} | user {} | password-set {} | public {}",
        status.enabled,
        status.server.unwrap_or("none"),
        status.device.unwrap_or("none"),
        status.mount,
        status.username,
        status.password_set,
        status.public
    );
    Ok(())
}

pub fn run_icecast_test(config_path: &Path) -> Result<()> {
    let config = ensure_icecast_ready(config_path)?;
    let server = config.server.as_deref().expect("validated Icecast server");
    let endpoint = parse_server_endpoint(server)?;
    let mut addrs = (endpoint.host.as_str(), endpoint.port)
        .to_socket_addrs()
        .with_context(|| format!("Failed to resolve Icecast server {}", endpoint.host))?;
    let Some(addr) = addrs.next() else {
        bail!(
            "No socket addresses found for Icecast server {}",
            endpoint.host
        );
    };

    TcpStream::connect_timeout(&addr, Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .with_context(|| format!("Failed to connect to Icecast server {server}"))?;
    println!("Icecast connection ok: {server} -> {addr}");
    Ok(())
}

pub fn run_icecast_devices() -> Result<()> {
    let output = Command::new("pactl")
        .args(["list", "short", "sources"])
        .output()
        .context("Failed to run pactl. Is PipeWire/PulseAudio installed?")?;
    if !output.status.success() {
        bail!("pactl list short sources failed");
    }

    let raw = String::from_utf8(output.stdout).context("pactl output was not UTF-8")?;
    for line in raw.lines() {
        let mut columns = line.split_whitespace();
        let _id = columns.next();
        let Some(name) = columns.next() else {
            continue;
        };
        if name.ends_with(".monitor") {
            println!("{name}");
        }
    }
    Ok(())
}

pub fn run_icecast_set_device(config_path: &Path, device: &str) -> Result<()> {
    validate_device(device)?;
    update_app_config(config_path, |config| {
        config.icecast.device = Some(device.to_string());
        Ok(())
    })
    .context("Failed to update Icecast device")?;
    println!("Icecast device set to {device}");
    Ok(())
}

pub fn run_icecast_start(config_path: &Path) -> Result<()> {
    gst::init().context("Failed to initialize GStreamer")?;
    let stream = start_icecast_device_stream(config_path)?;
    let Some(stream) = stream else {
        bail!("Icecast is disabled");
    };
    println!("Icecast device stream running. Press Ctrl+C to stop.");
    wait_for_icecast_stream(stream)
}

pub fn run_icecast_stream(config_path: &Path, source: &str) -> Result<()> {
    gst::init().context("Failed to initialize GStreamer")?;

    let config = ensure_icecast_ready(config_path)?;
    let server = config.server.as_deref().expect("validated Icecast server");
    let endpoint = parse_server_endpoint(server)?;
    let source_uri = source_to_uri(source)?;
    let password = config
        .password
        .as_deref()
        .expect("validated Icecast password");
    let streamname = config.name.as_deref().unwrap_or("Radio FM");

    let mut pipeline_args = vec![
        "uridecodebin".to_string(),
        format!("uri={source_uri}"),
        "!".to_string(),
        "audioconvert".to_string(),
        "!".to_string(),
        "audioresample".to_string(),
        "!".to_string(),
        "vorbisenc".to_string(),
        "!".to_string(),
        "oggmux".to_string(),
        "!".to_string(),
        "shout2send".to_string(),
        format!("ip={}", endpoint.host),
        format!("port={}", endpoint.port),
        format!("mount={}", config.mount),
        format!("username={}", config.username),
        format!("password={password}"),
        format!("streamname={streamname}"),
        format!("public={}", config.public),
        "sync=true".to_string(),
        "protocol=http".to_string(),
    ];
    if let Some(description) = config
        .description
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        pipeline_args.push(format!("description={description}"));
    }
    if let Some(genre) = config.genre.as_deref().filter(|value| !value.is_empty()) {
        pipeline_args.push(format!("genre={genre}"));
    }
    let pipeline_arg_refs = pipeline_args.iter().map(String::as_str).collect::<Vec<_>>();
    let pipeline = gst::parse::launchv(&pipeline_arg_refs)
        .context("Failed to build Icecast streaming pipeline")?;

    println!(
        "Streaming {source} to {}{} as {}",
        server, config.mount, config.username
    );
    pipeline
        .set_state(gst::State::Playing)
        .context("Failed to start Icecast stream")?;

    let bus = pipeline.bus().context("Pipeline has no message bus")?;
    loop {
        let Some(message) = bus.timed_pop(gst::ClockTime::NONE) else {
            continue;
        };
        use gst::MessageView;
        match message.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                let src = err
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                pipeline
                    .set_state(gst::State::Null)
                    .context("Failed to stop Icecast stream after error")?;
                bail!("Icecast stream error from {src}: {}", err.error());
            }
            _ => {}
        }
    }

    pipeline
        .set_state(gst::State::Null)
        .context("Failed to stop Icecast stream")?;
    println!("Icecast stream completed");
    Ok(())
}

pub struct IcecastStream {
    pipeline: gst::Element,
}

pub fn start_icecast_device_stream(config_path: &Path) -> Result<Option<IcecastStream>> {
    let config = load_app_config(config_path)?.icecast;
    if !config.enabled {
        return Ok(None);
    }

    ensure_icecast_config_ready(&config)?;
    let Some(device) = config.device.as_deref() else {
        bail!("Configure Icecast device first with `icecast set-device <monitor-device>`");
    };
    validate_device(device)?;

    let pipeline = build_icecast_device_pipeline(&config, device)?;
    pipeline
        .set_state(gst::State::Playing)
        .context("Failed to start Icecast device stream")?;
    println!(
        "Icecast device stream started: {} -> {}{}",
        device,
        config.server.as_deref().unwrap_or(""),
        config.mount
    );
    Ok(Some(IcecastStream { pipeline }))
}

pub fn poll_icecast_stream(stream: &mut Option<IcecastStream>) {
    let Some(active) = stream else {
        return;
    };
    let Some(bus) = active.pipeline.bus() else {
        eprintln!("Stopping Icecast stream because it has no message bus");
        stop_icecast_stream(stream);
        return;
    };

    while let Some(message) = bus.timed_pop(gst::ClockTime::ZERO) {
        use gst::MessageView;
        match message.view() {
            MessageView::Eos(..) => {
                println!("Icecast stream ended");
                stop_icecast_stream(stream);
                break;
            }
            MessageView::Error(err) => {
                let src = err
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                eprintln!("Icecast stream error from {src}: {}", err.error());
                stop_icecast_stream(stream);
                break;
            }
            _ => {}
        }
    }
}

pub fn stop_icecast_stream(stream: &mut Option<IcecastStream>) {
    if let Some(active) = stream.take() {
        let _ = active.pipeline.set_state(gst::State::Null);
    }
}

fn ensure_icecast_ready(config_path: &Path) -> Result<IcecastConfig> {
    let config = load_app_config(config_path)?.icecast;
    ensure_icecast_config_ready(&config)?;
    Ok(config)
}

fn ensure_icecast_config_ready(config: &IcecastConfig) -> Result<()> {
    let Some(server) = config.server.as_deref() else {
        bail!("Configure Icecast server first");
    };
    normalize_server(server)?;
    validate_mount(&config.mount)?;
    if config.username.trim().is_empty() {
        bail!("Configure Icecast username first");
    }
    if config.password.as_deref().is_none_or(str::is_empty) {
        bail!("Configure Icecast password first");
    }
    Ok(())
}

fn build_icecast_device_pipeline(config: &IcecastConfig, device: &str) -> Result<gst::Element> {
    let server = config.server.as_deref().expect("validated Icecast server");
    let endpoint = parse_server_endpoint(server)?;
    let password = config
        .password
        .as_deref()
        .expect("validated Icecast password");
    let streamname = config.name.as_deref().unwrap_or("Radio FM");

    let mut pipeline_args = vec![
        "pulsesrc".to_string(),
        format!("device={device}"),
        "client-name=radio-fm-icecast".to_string(),
        "do-timestamp=true".to_string(),
        "!".to_string(),
        "audioconvert".to_string(),
        "!".to_string(),
        "audioresample".to_string(),
        "!".to_string(),
        "vorbisenc".to_string(),
        "!".to_string(),
        "oggmux".to_string(),
        "!".to_string(),
        "shout2send".to_string(),
        format!("ip={}", endpoint.host),
        format!("port={}", endpoint.port),
        format!("mount={}", config.mount),
        format!("username={}", config.username),
        format!("password={password}"),
        format!("streamname={streamname}"),
        format!("public={}", config.public),
        "sync=true".to_string(),
        "protocol=http".to_string(),
    ];
    if let Some(description) = config
        .description
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        pipeline_args.push(format!("description={description}"));
    }
    if let Some(genre) = config.genre.as_deref().filter(|value| !value.is_empty()) {
        pipeline_args.push(format!("genre={genre}"));
    }

    let pipeline_arg_refs = pipeline_args.iter().map(String::as_str).collect::<Vec<_>>();
    gst::parse::launchv(&pipeline_arg_refs).context("Failed to build Icecast device pipeline")
}

fn wait_for_icecast_stream(stream: IcecastStream) -> Result<()> {
    let bus = stream
        .pipeline
        .bus()
        .context("Pipeline has no message bus")?;
    loop {
        let Some(message) = bus.timed_pop(gst::ClockTime::NONE) else {
            continue;
        };
        use gst::MessageView;
        match message.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                let src = err
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                stream
                    .pipeline
                    .set_state(gst::State::Null)
                    .context("Failed to stop Icecast stream after error")?;
                bail!("Icecast stream error from {src}: {}", err.error());
            }
            _ => {}
        }
    }

    stream
        .pipeline
        .set_state(gst::State::Null)
        .context("Failed to stop Icecast stream")?;
    Ok(())
}

fn source_to_uri(source: &str) -> Result<String> {
    if is_remote_media_source(source) || source.starts_with("file://") {
        return Ok(source.to_string());
    }

    let absolute = PathBuf::from(source)
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize source {source}"))?;
    gst::glib::filename_to_uri(absolute, None)
        .map(|uri| uri.to_string())
        .context("Failed to convert source path into URI")
}

fn normalize_server(server: &str) -> Result<String> {
    let trimmed = server.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        bail!("Icecast server cannot be empty");
    }
    parse_server_endpoint(trimmed)?;
    Ok(trimmed.to_string())
}

fn validate_mount(mount: &str) -> Result<()> {
    if !mount.starts_with('/') {
        bail!("Icecast mount must start with /");
    }
    if mount.trim() != mount || mount.len() == 1 {
        bail!("Icecast mount must be a non-empty path like /radio.ogg");
    }
    Ok(())
}

fn validate_device(device: &str) -> Result<()> {
    let device = device.trim();
    if device.is_empty() {
        bail!("Icecast device cannot be empty");
    }
    if !device.ends_with(".monitor") {
        bail!("Use a monitor source device, for example: alsa_output.name.monitor");
    }
    Ok(())
}

struct ServerEndpoint {
    host: String,
    port: u16,
}

fn parse_server_endpoint(server: &str) -> Result<ServerEndpoint> {
    let without_scheme = server
        .strip_prefix("http://")
        .or_else(|| server.strip_prefix("https://"))
        .unwrap_or(server);
    let authority = without_scheme
        .split('/')
        .next()
        .filter(|value| !value.is_empty())
        .context("Icecast server must include a host")?;
    let authority = authority
        .rsplit('@')
        .next()
        .context("Icecast server must include a host")?;

    if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() {
            bail!("Icecast server host cannot be empty");
        }
        let port = port
            .parse::<u16>()
            .with_context(|| format!("Invalid Icecast server port: {port}"))?;
        return Ok(ServerEndpoint {
            host: host.trim_matches(['[', ']']).to_string(),
            port,
        });
    }

    Ok(ServerEndpoint {
        host: authority.to_string(),
        port: DEFAULT_ICECAST_PORT,
    })
}
