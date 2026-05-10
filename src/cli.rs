use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::types::{DEFAULT_SCHEDULE_FILE, DEFAULT_SERVICE_SOCKET, DEFAULT_VOLUME};

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
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
    Gui,
}

#[derive(Subcommand, Debug)]
pub enum ScheduleCommands {
    Add {
        file: PathBuf,
        #[arg(long)]
        at: String,
        #[arg(long, default_value_t = 5)]
        fade_in: u64,
        #[arg(long, default_value_t = 5)]
        fade_out: u64,
        #[arg(long, default_value_t = DEFAULT_VOLUME)]
        volume: f64,
        #[arg(long, default_value_t = false)]
        mute: bool,
        #[arg(long, default_value = DEFAULT_SCHEDULE_FILE)]
        db: PathBuf,
    },
    List {
        #[arg(long, default_value = DEFAULT_SCHEDULE_FILE)]
        db: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long, default_value = DEFAULT_SCHEDULE_FILE)]
        db: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommands {
    Run {
        #[arg(long, default_value = DEFAULT_SCHEDULE_FILE)]
        db: PathBuf,
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
