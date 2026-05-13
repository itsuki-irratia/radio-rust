use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

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
pub struct PlaybackConfig {
    #[serde(default = "default_fade_in_secs")]
    pub default_fade_in_secs: u64,
    #[serde(default = "default_fade_out_secs")]
    pub default_fade_out_secs: u64,
    #[serde(default = "default_volume")]
    pub default_volume: f64,
    #[serde(default)]
    pub default_mute: bool,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            default_fade_in_secs: DEFAULT_FADE_IN_SECS,
            default_fade_out_secs: DEFAULT_FADE_OUT_SECS,
            default_volume: DEFAULT_VOLUME,
            default_mute: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub playback: PlaybackConfig,
    #[serde(default)]
    pub streams: StreamDb,
    #[serde(default)]
    pub time_signal: TimeSignalConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            playback: PlaybackConfig::default(),
            streams: StreamDb::default(),
            time_signal: TimeSignalConfig::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LiveOverrides {
    pub volume: Option<f64>,
    pub mute: Option<bool>,
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

pub fn default_fade_in_secs() -> u64 {
    DEFAULT_FADE_IN_SECS
}

pub fn default_fade_out_secs() -> u64 {
    DEFAULT_FADE_OUT_SECS
}

pub fn default_enabled() -> bool {
    true
}
