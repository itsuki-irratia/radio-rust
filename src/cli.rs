use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::types::{DEFAULT_FADE_IN_SECS, DEFAULT_FADE_OUT_SECS, DEFAULT_SERVICE_SOCKET};

#[derive(Parser, Debug)]
#[command(author, version, about = "Radio FM starter CLI + GUI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Scan {
        folder: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommands,
    },
    Streams {
        #[command(subcommand)]
        command: StreamsCommands,
    },
    #[command(name = "time-signal", aliases = ["greenwich", "greenwitch"])]
    TimeSignal {
        #[command(subcommand)]
        command: TimeSignalCommands,
    },
    Cron {
        #[command(subcommand)]
        command: CronCommands,
    },
    Icecast {
        #[command(subcommand)]
        command: IcecastCommands,
    },
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
    Gui,
}

#[derive(Subcommand, Debug)]
pub enum StreamsCommands {
    List {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum TimeSignalCommands {
    SetAudio {
        source: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Enable {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Disable {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    DisableDuringStreams {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    EnableDuringStreams {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Streams {
        enabled: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Status {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum CronCommands {
    Add {
        file: PathBuf,
        #[arg(long)]
        expr: String,
        #[arg(long)]
        fade_in: Option<u64>,
        #[arg(long)]
        fade_out: Option<u64>,
        #[arg(long)]
        volume: Option<f64>,
        #[arg(long, default_value_t = false)]
        mute: bool,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    List {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Remove {
        id: u64,
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum IcecastCommands {
    Configure {
        #[arg(long)]
        server: String,
        #[arg(long)]
        mount: String,
        #[arg(long, default_value = "source")]
        username: String,
        #[arg(long)]
        password: String,
        #[arg(long)]
        device: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        genre: Option<String>,
        #[arg(long, default_value_t = false)]
        public: bool,
        #[arg(long, default_value = "true")]
        enabled: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Enable {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Disable {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Status {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    Test {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Devices,
    SetDevice {
        device: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Start {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Stream {
        source: String,
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ScheduleCommands {
    Add {
        file: PathBuf,
        #[arg(long)]
        at: String,
        #[arg(long)]
        fade_in: Option<u64>,
        #[arg(long)]
        fade_out: Option<u64>,
        #[arg(long)]
        volume: Option<f64>,
        #[arg(long, default_value_t = false)]
        mute: bool,
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
    },
    List {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        day: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
    },
    Run {
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommands {
    Run {
        #[arg(long)]
        db: Option<PathBuf>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Play {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Status {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    SetVolume {
        value: f64,
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    FadeIn {
        #[arg(default_value_t = DEFAULT_FADE_IN_SECS)]
        seconds: u64,
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    FadeOut {
        #[arg(default_value_t = DEFAULT_FADE_OUT_SECS)]
        seconds: u64,
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Mute {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Unmute {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Skip {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Stop {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
    Shutdown {
        #[arg(long, default_value = DEFAULT_SERVICE_SOCKET)]
        socket: PathBuf,
    },
}
