mod cli;
mod cron;
mod gui;
mod playback;
mod schedule;
mod service;
mod types;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Commands, CronCommands, ScheduleCommands, ServiceCommands};
use crate::cron::{run_cron_add, run_cron_list, run_cron_remove};
use crate::schedule::{
    run_scan, run_schedule_add, run_schedule_list, run_schedule_run, validate_volume,
};
use crate::service::{run_service, send_service_command};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan { folder, json } => run_scan(&folder, json),
        Commands::Schedule { command } => run_schedule_command(command),
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
        ScheduleCommands::List { db, json } => run_schedule_list(&db, json),
        ScheduleCommands::Run { db } => run_schedule_run(&db),
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
