mod cli;
mod config;
mod cron;
mod icecast;
mod mcp;
mod playback;
mod schedule;
mod service;
mod streams;
mod time_signal;
mod types;

use anyhow::{Result, bail};
use clap::Parser;

use crate::cli::{
    Cli, Commands, CronCommands, IcecastCommands, McpCommands, McpTokenCommands, ScheduleCommands,
    ServiceCommands, StreamsCommands, TimeSignalCommands,
};
use crate::config::{
    load_app_config, resolve_config_path, resolve_db_path, resolve_service_socket_path,
};
use crate::cron::{run_cron_add, run_cron_list, run_cron_remove};
use crate::icecast::{
    IcecastConfigure, run_icecast_configure, run_icecast_devices, run_icecast_disable,
    run_icecast_enable, run_icecast_set_device, run_icecast_start, run_icecast_status,
    run_icecast_stream, run_icecast_test,
};
use crate::mcp::{
    run_mcp_configure, run_mcp_disable, run_mcp_enable, run_mcp_server, run_mcp_status,
    run_mcp_token_create, run_mcp_token_list, run_mcp_token_revoke,
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
use crate::types::{AppConfig, PlaybackOptions};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan { folder, json } => run_scan(&folder, json),
        Commands::Schedule { command } => run_schedule_command(command),
        Commands::Streams { command } => run_streams_command(command),
        Commands::TimeSignal { command } => run_time_signal_command(command),
        Commands::Cron { command } => run_cron_command(command),
        Commands::Icecast { command } => run_icecast_command(command),
        Commands::Mcp { command } => run_mcp_command(command),
        Commands::Service { command } => run_service_command(command),
    }
}

fn run_mcp_command(command: McpCommands) -> Result<()> {
    match command {
        McpCommands::Configure {
            port,
            enabled,
            config,
        } => run_mcp_configure(
            &resolve_config_path(config)?,
            port,
            parse_bool_arg(&enabled)?,
        ),
        McpCommands::Enable { config } => run_mcp_enable(&resolve_config_path(config)?),
        McpCommands::Disable { config } => run_mcp_disable(&resolve_config_path(config)?),
        McpCommands::Status { config, json } => run_mcp_status(&resolve_config_path(config)?, json),
        McpCommands::Run { config, port } => run_mcp_server(&resolve_config_path(config)?, port),
        McpCommands::Token { command } => run_mcp_token_command(command),
    }
}

fn run_mcp_token_command(command: McpTokenCommands) -> Result<()> {
    match command {
        McpTokenCommands::Create {
            name,
            scope,
            config,
        } => run_mcp_token_create(&resolve_config_path(config)?, &name, &scope),
        McpTokenCommands::List { config, json } => {
            run_mcp_token_list(&resolve_config_path(config)?, json)
        }
        McpTokenCommands::Revoke { id, config } => {
            run_mcp_token_revoke(&resolve_config_path(config)?, &id)
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
            let playback = resolve_playback_options(config, fade_in, fade_out, volume, mute)?;
            run_schedule_add(&db, &file, &at, playback)
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
            let playback = resolve_playback_options(config, fade_in, fade_out, volume, mute)?;
            run_cron_add(&db, &file, &expr, playback)
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
            &resolve_service_socket_path(socket)?,
        ),
        command => run_service_control_command(command),
    }
}

fn resolve_playback_options(
    config_path: Option<std::path::PathBuf>,
    fade_in: Option<u64>,
    fade_out: Option<u64>,
    volume: Option<f64>,
    mute: bool,
) -> Result<PlaybackOptions> {
    let config = load_app_config(&resolve_config_path(config_path)?)?;
    Ok(playback_options_from_config(
        &config, fade_in, fade_out, volume, mute,
    ))
}

fn playback_options_from_config(
    config: &AppConfig,
    fade_in: Option<u64>,
    fade_out: Option<u64>,
    volume: Option<f64>,
    mute: bool,
) -> PlaybackOptions {
    PlaybackOptions {
        fade_in_secs: fade_in.unwrap_or(config.fade.duration),
        fade_out_secs: fade_out.unwrap_or(config.fade.duration),
        volume: volume.unwrap_or(config.playback.default_volume),
        mute: mute || config.playback.default_mute,
    }
}

fn run_service_control_command(command: ServiceCommands) -> Result<()> {
    let (socket, request) = match command {
        ServiceCommands::Play { socket } => (socket, "play".to_owned()),
        ServiceCommands::Status { socket } => (socket, "status".to_owned()),
        ServiceCommands::SetVolume { value, socket } => {
            validate_volume(value)?;
            (socket, format!("set-volume {value}"))
        }
        ServiceCommands::FadeIn { seconds, socket } => (socket, format!("fade-in {seconds}")),
        ServiceCommands::FadeOut { seconds, socket } => (socket, format!("fade-out {seconds}")),
        ServiceCommands::Mute { socket } => (socket, "mute on".to_owned()),
        ServiceCommands::Unmute { socket } => (socket, "mute off".to_owned()),
        ServiceCommands::Skip { socket } => (socket, "skip".to_owned()),
        ServiceCommands::Stop { socket } => (socket, "stop".to_owned()),
        ServiceCommands::Shutdown { socket } => (socket, "shutdown".to_owned()),
        ServiceCommands::Run { .. } => unreachable!("service run is handled separately"),
    };
    let response = send_service_command(&resolve_service_socket_path(socket)?, &request)?;
    print!("{response}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::playback_options_from_config;
    use crate::types::AppConfig;

    #[test]
    fn playback_options_use_config_defaults() {
        let mut config = AppConfig::default();
        config.fade.duration = 9;
        config.playback.default_volume = 0.4;
        config.playback.default_mute = true;

        let options = playback_options_from_config(&config, None, None, None, false);

        assert_eq!(options.fade_in_secs, 9);
        assert_eq!(options.fade_out_secs, 9);
        assert_eq!(options.volume, 0.4);
        assert!(options.mute);
    }

    #[test]
    fn playback_options_allow_explicit_values() {
        let options =
            playback_options_from_config(&AppConfig::default(), Some(1), Some(2), Some(0.3), true);

        assert_eq!(options.fade_in_secs, 1);
        assert_eq!(options.fade_out_secs, 2);
        assert_eq!(options.volume, 0.3);
        assert!(options.mute);
    }
}
