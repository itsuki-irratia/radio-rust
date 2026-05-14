mod cli;
mod config;
mod cron;
mod gui;
mod icecast;
mod playback;
mod schedule;
mod service;
mod streams;
mod time_signal;
mod types;

use anyhow::{Result, bail};
use clap::Parser;

use crate::cli::{
    Cli, Commands, CronCommands, IcecastCommands, ScheduleCommands, ServiceCommands,
    StreamsCommands, TimeSignalCommands,
};
use crate::config::{load_app_config, resolve_config_path, resolve_db_path};
use crate::cron::{run_cron_add, run_cron_list, run_cron_remove};
use crate::icecast::{
    IcecastConfigure, run_icecast_configure, run_icecast_devices, run_icecast_disable,
    run_icecast_enable, run_icecast_set_device, run_icecast_start, run_icecast_status,
    run_icecast_stream, run_icecast_test,
};
use crate::schedule::{
    run_scan, run_schedule_add, run_schedule_list, run_schedule_run, validate_volume,
};
use crate::service::{run_service, send_service_command};
use crate::streams::{run_streams_add, run_streams_list};
use crate::time_signal::{
    run_time_signal_disable, run_time_signal_disable_during_streams, run_time_signal_enable,
    run_time_signal_enable_during_streams, run_time_signal_set_audio, run_time_signal_set_streams,
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
        Commands::Icecast { command } => run_icecast_command(command),
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
            config,
        } => {
            let db = resolve_db_path(db)?;
            let config = load_app_config(&resolve_config_path(config)?)?;
            run_schedule_add(
                &db,
                &file,
                &at,
                fade_in.unwrap_or(config.fade.duration),
                fade_out.unwrap_or(config.fade.duration),
                volume.unwrap_or(config.playback.default_volume),
                mute || config.playback.default_mute,
            )
        }
        ScheduleCommands::List {
            db,
            json,
            day,
            from,
            to,
        } => run_schedule_list(
            &resolve_db_path(db)?,
            json,
            day.as_deref(),
            from.as_deref(),
            to.as_deref(),
        ),
        ScheduleCommands::Run { db } => run_schedule_run(&resolve_db_path(db)?),
    }
}

fn run_streams_command(command: StreamsCommands) -> Result<()> {
    match command {
        StreamsCommands::Add {
            slug,
            name,
            url,
            config,
        } => run_streams_add(&resolve_config_path(config)?, &slug, &name, &url),
        StreamsCommands::List { config, json } => {
            run_streams_list(&resolve_config_path(config)?, json)
        }
    }
}

fn run_time_signal_command(command: TimeSignalCommands) -> Result<()> {
    match command {
        TimeSignalCommands::SetAudio { source, config } => {
            run_time_signal_set_audio(&resolve_config_path(config)?, &source)
        }
        TimeSignalCommands::Enable { config } => {
            run_time_signal_enable(&resolve_config_path(config)?)
        }
        TimeSignalCommands::Disable { config } => {
            run_time_signal_disable(&resolve_config_path(config)?)
        }
        TimeSignalCommands::DisableDuringStreams { config } => {
            run_time_signal_disable_during_streams(&resolve_config_path(config)?)
        }
        TimeSignalCommands::EnableDuringStreams { config } => {
            run_time_signal_enable_during_streams(&resolve_config_path(config)?)
        }
        TimeSignalCommands::Streams { enabled, config } => {
            let enabled = parse_bool_arg(&enabled)?;
            run_time_signal_set_streams(&resolve_config_path(config)?, enabled)?;
            println!("Greenwich time signal streams set to {enabled}");
            Ok(())
        }
        TimeSignalCommands::Status { config, json } => {
            run_time_signal_status(&resolve_config_path(config)?, json)
        }
    }
}

fn parse_bool_arg(value: &str) -> Result<bool> {
    match value {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => bail!("Use true or false"),
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
            config,
        } => {
            let db = resolve_db_path(db)?;
            let config = load_app_config(&resolve_config_path(config)?)?;
            run_cron_add(
                &db,
                &file,
                &expr,
                fade_in.unwrap_or(config.fade.duration),
                fade_out.unwrap_or(config.fade.duration),
                volume.unwrap_or(config.playback.default_volume),
                mute || config.playback.default_mute,
            )
        }
        CronCommands::List { db, json } => run_cron_list(&resolve_db_path(db)?, json),
        CronCommands::Remove { id, db } => run_cron_remove(&resolve_db_path(db)?, id),
    }
}

fn run_icecast_command(command: IcecastCommands) -> Result<()> {
    match command {
        IcecastCommands::Configure {
            server,
            mount,
            username,
            password,
            device,
            name,
            description,
            genre,
            public,
            enabled,
            config,
        } => {
            let enabled = parse_bool_arg(&enabled)?;
            run_icecast_configure(
                &resolve_config_path(config)?,
                IcecastConfigure {
                    server,
                    mount,
                    username,
                    password,
                    device,
                    name,
                    description,
                    genre,
                    public,
                    enabled,
                },
            )
        }
        IcecastCommands::Enable { config } => run_icecast_enable(&resolve_config_path(config)?),
        IcecastCommands::Disable { config } => run_icecast_disable(&resolve_config_path(config)?),
        IcecastCommands::Status { config, json } => {
            run_icecast_status(&resolve_config_path(config)?, json)
        }
        IcecastCommands::Test { config } => run_icecast_test(&resolve_config_path(config)?),
        IcecastCommands::Devices => run_icecast_devices(),
        IcecastCommands::SetDevice { device, config } => {
            run_icecast_set_device(&resolve_config_path(config)?, &device)
        }
        IcecastCommands::Start { config } => run_icecast_start(&resolve_config_path(config)?),
        IcecastCommands::Stream { source, config } => {
            run_icecast_stream(&resolve_config_path(config)?, &source)
        }
    }
}

fn run_service_command(command: ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::Run { db, config, socket } => run_service(
            &resolve_db_path(db)?,
            &resolve_config_path(config)?,
            &socket,
        ),
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
        ServiceCommands::FadeIn { seconds, socket } => {
            let response = send_service_command(&socket, &format!("fade-in {seconds}"))?;
            print!("{response}");
            Ok(())
        }
        ServiceCommands::FadeOut { seconds, socket } => {
            let response = send_service_command(&socket, &format!("fade-out {seconds}"))?;
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
