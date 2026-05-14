use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["mp3", "aac", "flac", "ogg", "opus", "wav", "m4a", "xspf"];
pub const DEFAULT_CONFIG_DIR_NAME: &str = "radio-rust";
pub const DEFAULT_CONFIG_FILE_NAME: &str = "radio-rust.json";
pub const DEFAULT_SCHEDULE_DB_FILE_NAME: &str = "schedule.sqlite";
pub const FADE_TICK_MS: u64 = 200;
pub const DEFAULT_SERVICE_SOCKET: &str = "/tmp/radio-fm.sock";
pub const SERVICE_TICK_MS: u64 = 250;
pub const DEFAULT_VOLUME: f64 = 1.0;
pub const DEFAULT_FADE_IN_SECS: u64 = 5;
pub const DEFAULT_FADE_OUT_SECS: u64 = 5;

#[derive(Serialize)]
pub struct ScanResult {
    pub folder: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub id: u64,
    pub file: String,
    pub at: DateTime<Local>,
    pub fade_in_secs: u64,
    pub fade_out_secs: u64,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub mute: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ScheduleDb {
    pub entries: Vec<ScheduleEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronEntry {
    pub id: u64,
    pub expression: String,
    pub file: String,
    pub fade_in_secs: u64,
    pub fade_out_secs: u64,
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default)]
    pub mute: bool,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CronDb {
    pub entries: Vec<CronEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEntry {
    pub id: u64,
    pub slug: String,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct StreamDb {
    pub entries: Vec<StreamEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeSignalConfig {
    pub enabled: bool,
    pub source: Option<String>,
    pub streams: bool,
}

impl<'de> Deserialize<'de> for TimeSignalConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct TimeSignalConfigInput {
            #[serde(default)]
            enabled: bool,
            #[serde(default)]
            source: Option<String>,
            streams: Option<bool>,
            #[serde(default)]
            skip_during_streams: Option<bool>,
        }

        let input = TimeSignalConfigInput::deserialize(deserializer)?;
        Ok(Self {
            enabled: input.enabled,
            source: input.source,
            streams: input
                .streams
                .unwrap_or_else(|| input.skip_during_streams.map(|skip| !skip).unwrap_or(true)),
        })
    }
}

impl Default for TimeSignalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            source: None,
            streams: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FadeConfig {
    #[serde(default = "default_fade_duration_secs")]
    pub duration: u64,
}

impl Default for FadeConfig {
    fn default() -> Self {
        Self {
            duration: DEFAULT_FADE_IN_SECS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackConfig {
    #[serde(default = "default_volume")]
    pub default_volume: f64,
    #[serde(default)]
    pub default_mute: bool,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            default_volume: DEFAULT_VOLUME,
            default_mute: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IcecastConfig {
    #[serde(default)]
    pub enabled: bool,
    pub server: Option<String>,
    pub device: Option<String>,
    #[serde(default = "default_icecast_mount")]
    pub mount: String,
    #[serde(default = "default_icecast_username")]
    pub username: String,
    pub password: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    #[serde(default)]
    pub public: bool,
}

impl Default for IcecastConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: None,
            device: None,
            mount: default_icecast_mount(),
            username: default_icecast_username(),
            password: None,
            name: None,
            description: None,
            genre: None,
            public: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub fade: FadeConfig,
    #[serde(default)]
    pub playback: PlaybackConfig,
    #[serde(default)]
    pub streams: StreamDb,
    #[serde(default)]
    pub time_signal: TimeSignalConfig,
    #[serde(default)]
    pub icecast: IcecastConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            fade: FadeConfig::default(),
            playback: PlaybackConfig::default(),
            streams: StreamDb::default(),
            time_signal: TimeSignalConfig::default(),
            icecast: IcecastConfig::default(),
        }
    }
}

impl<'de> Deserialize<'de> for AppConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct AppConfigInput {
            #[serde(default)]
            fade: Option<FadeConfig>,
            #[serde(default)]
            playback: PlaybackConfigInput,
            #[serde(default)]
            streams: StreamDb,
            #[serde(default)]
            time_signal: TimeSignalConfig,
            #[serde(default)]
            icecast: IcecastConfig,
        }

        #[derive(Default, Deserialize)]
        struct PlaybackConfigInput {
            #[serde(default = "default_volume")]
            default_volume: f64,
            #[serde(default)]
            default_mute: bool,
            #[serde(default)]
            default_fade_in_secs: Option<u64>,
            #[serde(default)]
            default_fade_out_secs: Option<u64>,
        }

        let input = AppConfigInput::deserialize(deserializer)?;
        let fade_duration = input
            .fade
            .map(|fade| fade.duration)
            .or(input.playback.default_fade_in_secs)
            .or(input.playback.default_fade_out_secs)
            .unwrap_or(DEFAULT_FADE_IN_SECS);

        Ok(Self {
            fade: FadeConfig {
                duration: fade_duration,
            },
            playback: PlaybackConfig {
                default_volume: input.playback.default_volume,
                default_mute: input.playback.default_mute,
            },
            streams: input.streams,
            time_signal: input.time_signal,
            icecast: input.icecast,
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LiveOverrides {
    pub volume: Option<f64>,
    pub mute: Option<bool>,
    pub fade_request: Option<LiveVolumeFadeRequest>,
    pub active_fade: Option<LiveVolumeFade>,
    pub fade_return_volume: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
pub struct LiveVolumeFadeRequest {
    pub direction: LiveVolumeFadeDirection,
    pub duration: Duration,
}

#[derive(Debug, Clone, Copy)]
pub enum LiveVolumeFadeDirection {
    In,
    Out,
}

#[derive(Debug, Clone, Copy)]
pub struct LiveVolumeFade {
    pub started_at: Instant,
    pub duration: Duration,
    pub from_volume: f64,
    pub to_volume: f64,
}

#[derive(Debug, Clone)]
pub struct ServiceState {
    pub now_playing: Option<String>,
    pub now_playing_id: Option<u64>,
    pub audio_enabled: bool,
}

impl ServiceState {
    pub fn new() -> Self {
        Self {
            now_playing: None,
            now_playing_id: None,
            audio_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceDirective {
    Continue,
    SkipCurrent,
    StopAudio,
    ReplaceCurrent,
    StopService,
}

pub fn default_volume() -> f64 {
    DEFAULT_VOLUME
}

pub fn default_fade_duration_secs() -> u64 {
    DEFAULT_FADE_IN_SECS
}

pub fn default_icecast_mount() -> String {
    "/radio.ogg".to_string()
}

pub fn default_icecast_username() -> String {
    "source".to_string()
}

pub fn default_enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_playback_fade_defaults_migrate_to_fade_duration() {
        let raw = r#"{
            "playback": {
                "default_fade_in_secs": 7,
                "default_fade_out_secs": 9,
                "default_volume": 0.4,
                "default_mute": true
            }
        }"#;

        let config: AppConfig = serde_json::from_str(raw).expect("legacy config parses");

        assert_eq!(config.fade.duration, 7);
        assert_eq!(config.playback.default_volume, 0.4);
        assert!(config.playback.default_mute);
    }

    #[test]
    fn app_config_serializes_fade_separately_from_playback() {
        let config = AppConfig {
            fade: FadeConfig { duration: 5 },
            playback: PlaybackConfig {
                default_volume: 1.0,
                default_mute: false,
            },
            ..AppConfig::default()
        };

        let raw = serde_json::to_string_pretty(&config).expect("config serializes");

        assert!(raw.contains("\"fade\""));
        assert!(raw.contains("\"duration\": 5"));
        assert!(raw.contains("\"playback\""));
        assert!(raw.contains("\"default_volume\": 1.0"));
        assert!(!raw.contains("default_fade_in_secs"));
        assert!(!raw.contains("default_fade_out_secs"));
    }
}
