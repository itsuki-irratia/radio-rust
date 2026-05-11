use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["mp3", "aac", "flac", "ogg", "opus", "wav", "m4a", "xspf"];
pub const DEFAULT_SCHEDULE_DB: &str = "radio-fm-schedule.sqlite";
pub const FADE_TICK_MS: u64 = 200;
pub const DEFAULT_SERVICE_SOCKET: &str = "/tmp/radio-fm.sock";
pub const SERVICE_TICK_MS: u64 = 250;
pub const DEFAULT_VOLUME: f64 = 1.0;

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

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StreamDb {
    pub entries: Vec<StreamEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeSignalConfig {
    pub enabled: bool,
    pub source: Option<String>,
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

pub fn default_enabled() -> bool {
    true
}
