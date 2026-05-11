mod cli;
mod cron;
mod gui;
mod playback;
mod schedule;
mod service;
mod streams;
mod time_signal;
mod types;

use anyhow::Result;
use clap::Parser;

use crate::cli::{
    Cli, Commands, CronCommands, ScheduleCommands, ServiceCommands, StreamsCommands,
    TimeSignalCommands,
};
use crate::cron::{run_cron_add, run_cron_list, run_cron_remove};
use crate::schedule::{
    run_scan, run_schedule_add, run_schedule_list, run_schedule_run, validate_volume,
};
use crate::service::{run_service, send_service_command};
use crate::streams::run_streams_list;
use crate::time_signal::{
    run_time_signal_disable, run_time_signal_enable, run_time_signal_set_audio,
    run_time_signal_status,
};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan { folder, json } => run_scan(&folder, json),
        Commands::Schedule { command } => run_schedule_command(command),
        Commands::Streams { command } => run_streams_command(command),
        Commands::TimeSignal { command } => run_time_signal_command(command),
        Commands::Cron { command } => run_cron_command(command),
        Commands::Service { command } => run_service_command(command),
        Commands::Gui => {
            gui::run_gui();
            Ok(())
        }
    }
}

fn run_schedule_command(command: ScheduleCommands) -> Result<()> {
    match command {
        ScheduleCommands::Add {
            file,
            at,
            fade_in,
            fade_out,
            volume,
            mute,
            db,
        } => run_schedule_add(&db, &file, &at, fade_in, fade_out, volume, mute),
        ScheduleCommands::List {
            db,
            json,
            day,
            from,
            to,
        } => run_schedule_list(&db, json, day.as_deref(), from.as_deref(), to.as_deref()),
        ScheduleCommands::Run { db } => run_schedule_run(&db),
    }
}

fn run_streams_command(command: StreamsCommands) -> Result<()> {
    match command {
        StreamsCommands::List { db, json } => run_streams_list(&db, json),
    }
}

fn run_time_signal_command(command: TimeSignalCommands) -> Result<()> {
    match command {
        TimeSignalCommands::SetAudio { source, db } => run_time_signal_set_audio(&db, &source),
        TimeSignalCommands::Enable { db } => run_time_signal_enable(&db),
        TimeSignalCommands::Disable { db } => run_time_signal_disable(&db),
        TimeSignalCommands::Status { db, json } => run_time_signal_status(&db, json),
    }
}

fn run_cron_command(command: CronCommands) -> Result<()> {
    match command {
        CronCommands::Add {
            file,
            expr,
            fade_in,
            fade_out,
            volume,
            mute,
            db,
        } => run_cron_add(&db, &file, &expr, fade_in, fade_out, volume, mute),
        CronCommands::List { db, json } => run_cron_list(&db, json),
        CronCommands::Remove { id, db } => run_cron_remove(&db, id),
    }
}

fn run_service_command(command: ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::Run { db, socket } => run_service(&db, &socket),
        ServiceCommands::Play { socket } => {
            let response = send_service_command(&socket, "play")?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::Status { socket } => {
            let response = send_service_command(&socket, "status")?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::SetVolume { value, socket } => {
            validate_volume(value)?;
            let response = send_service_command(&socket, &format!("set-volume {value}"))?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::Mute { socket } => {
            let response = send_service_command(&socket, "mute on")?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::Unmute { socket } => {
            let response = send_service_command(&socket, "mute off")?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::Skip { socket } => {
            let response = send_service_command(&socket, "skip")?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::Stop { socket } => {
            let response = send_service_command(&socket, "stop")?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::Shutdown { socket } => {
            let response = send_service_command(&socket, "shutdown")?;
            print!("{response}");
            Ok(())
        }
    }
}
