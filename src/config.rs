use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::types::{
    AppConfig, DEFAULT_CONFIG_DIR_NAME, DEFAULT_CONFIG_FILE_NAME, DEFAULT_SCHEDULE_DB_FILE_NAME,
    DEFAULT_SERVICE_SOCKET_FILE_NAME,
};

pub fn default_config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join(DEFAULT_CONFIG_DIR_NAME))
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join(DEFAULT_CONFIG_FILE_NAME))
}

pub fn default_schedule_db_path() -> Result<PathBuf> {
    Ok(default_config_dir()?.join(DEFAULT_SCHEDULE_DB_FILE_NAME))
}

pub fn default_service_socket_path() -> Result<PathBuf> {
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|path| !path.is_empty()) {
        return Ok(PathBuf::from(runtime_dir).join(DEFAULT_SERVICE_SOCKET_FILE_NAME));
    }
    Ok(default_config_dir()?.join(DEFAULT_SERVICE_SOCKET_FILE_NAME))
}

pub fn resolve_service_socket_path(socket_path: Option<PathBuf>) -> Result<PathBuf> {
    socket_path.map_or_else(default_service_socket_path, Ok)
}

pub fn resolve_config_path(config_path: Option<PathBuf>) -> Result<PathBuf> {
    match config_path {
        Some(path) => Ok(path),
        None => default_config_path(),
    }
}

pub fn resolve_db_path(db_path: Option<PathBuf>) -> Result<PathBuf> {
    match db_path {
        Some(path) => Ok(path),
        None => default_schedule_db_path(),
    }
}

pub fn load_app_config(config_path: &Path) -> Result<AppConfig> {
    let mut needs_save = !config_path.exists();
    let config = if config_path.exists() {
        restrict_config_permissions(config_path)?;
        let raw = read_utf8_file_limited(config_path, MAX_CONFIG_FILE_BYTES, "configuration file")?;
        if raw.trim().is_empty() {
            needs_save = true;
            AppConfig::default()
        } else {
            if !raw.contains("\"icecast\"") || !raw.contains("\"mcp\"") {
                needs_save = true;
            }
            if !raw.contains("\"fade\"")
                || raw.contains("\"default_fade_in_secs\"")
                || raw.contains("\"default_fade_out_secs\"")
            {
                needs_save = true;
            }
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse config file {}", config_path.display()))?
        }
    } else {
        AppConfig::default()
    };

    if needs_save {
        save_app_config(config_path, &config)?;
    }

    Ok(config)
}

fn restrict_config_permissions(config_path: &Path) -> Result<()> {
    fs::set_permissions(config_path, fs::Permissions::from_mode(0o600)).with_context(|| {
        format!(
            "Failed to restrict permissions on config file {}",
            config_path.display()
        )
    })
}

pub fn save_app_config(config_path: &Path, config: &AppConfig) -> Result<()> {
    validate_app_config(config)?;
    if let Some(parent) = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(config).context("Failed to serialize app config")?;
    if raw.len() > MAX_CONFIG_FILE_BYTES {
        bail!(
            "Configuration exceeds the {} byte limit",
            MAX_CONFIG_FILE_BYTES
        );
    }
    write_config_atomically(config_path, &format!("{raw}\n"))?;
    Ok(())
}

pub fn read_utf8_file_limited(path: &Path, max_bytes: usize, description: &str) -> Result<String> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {description} {}", path.display()))?;
    let mut raw = String::new();
    file.take((max_bytes + 1) as u64)
        .read_to_string(&mut raw)
        .with_context(|| format!("Failed to read {description} {}", path.display()))?;
    if raw.len() > max_bytes {
        bail!(
            "{description} {} exceeds the {} byte limit",
            path.display(),
            max_bytes
        );
    }
    Ok(raw)
}

fn write_config_atomically(config_path: &Path, contents: &str) -> Result<()> {
    let parent = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Config path must end in a valid UTF-8 filename")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary_path = parent.join(format!(
        ".{filename}.{}.{}.tmp",
        process::id(),
        CONFIG_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed) ^ nonce as u64
    ));

    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary_path)
            .with_context(|| {
                format!(
                    "Failed to create temporary config file {}",
                    temporary_path.display()
                )
            })?;
        file.write_all(contents.as_bytes()).with_context(|| {
            format!(
                "Failed to write temporary config file {}",
                temporary_path.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "Failed to sync temporary config file {}",
                temporary_path.display()
            )
        })?;
        fs::rename(&temporary_path, config_path)
            .with_context(|| format!("Failed to replace config file {}", config_path.display()))?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

static CONFIG_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);
const CONFIG_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const CONFIG_LOCK_RETRY: Duration = Duration::from_millis(50);
pub const MAX_CONFIG_FILE_BYTES: usize = 4 * 1024 * 1024;

pub fn update_app_config(
    config_path: &Path,
    update: impl FnOnce(&mut AppConfig) -> Result<()>,
) -> Result<AppConfig> {
    let lock_file = lock_config_for_update(config_path)?;
    let result = (|| -> Result<AppConfig> {
        let mut config = load_app_config(config_path)?;
        update(&mut config)?;
        save_app_config(config_path, &config)?;
        Ok(config)
    })();
    let unlock_result = FileExt::unlock(&lock_file).context("Failed to unlock app config");
    match (result, unlock_result) {
        (Ok(config), Ok(())) => Ok(config),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn lock_config_for_update(config_path: &Path) -> Result<std::fs::File> {
    let parent = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
    let filename = config_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Config path must end in a valid UTF-8 filename")?;
    let lock_path = parent.join(format!(".{filename}.lock"));
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .with_context(|| format!("Failed to open config lock file {}", lock_path.display()))?;
    lock_file
        .set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| {
            format!(
                "Failed to restrict config lock file {}",
                lock_path.display()
            )
        })?;

    let started = Instant::now();
    loop {
        match lock_file.try_lock_exclusive() {
            Ok(()) => return Ok(lock_file),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= CONFIG_LOCK_TIMEOUT {
                    bail!(
                        "Timed out waiting to update config file {}",
                        config_path.display()
                    );
                }
                thread::sleep(CONFIG_LOCK_RETRY);
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Failed to lock config file {}", config_path.display())
                });
            }
        }
    }
}

fn validate_app_config(config: &AppConfig) -> Result<()> {
    if config.fade.duration > i64::MAX as u64 {
        bail!(
            "Invalid fade duration {}. Use a smaller value",
            config.fade.duration
        );
    }
    let volume = config.playback.default_volume;
    if !(0.0..=1.0).contains(&volume) {
        bail!("Invalid default volume {volume}. Use a value between 0.0 and 1.0");
    }
    if config.mcp.port == 0 {
        bail!("Invalid MCP port 0. Use a port between 1 and 65535");
    }
    for token in &config.mcp.tokens {
        if token.id.trim().is_empty() || token.name.trim().is_empty() || token.created_at.is_empty()
        {
            bail!("Invalid MCP token entry. Token id, name, and creation time are required");
        }
        if token.token_hash.len() != 64
            || !token
                .token_hash
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            bail!(
                "Invalid MCP token entry {}. Its token hash is malformed",
                token.id
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn saves_configuration_atomically_with_owner_only_permissions() {
        let directory = std::env::temp_dir().join(format!(
            "radio-fm-config-test-{}-{}",
            process::id(),
            CONFIG_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("radio-rust.json");

        save_app_config(&path, &AppConfig::default()).expect("save config");

        let metadata = fs::metadata(&path).expect("read config metadata");
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(load_app_config(&path).expect("load config").mcp.port, 3333);

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn rejects_an_oversized_configuration_before_parsing() {
        let directory = std::env::temp_dir().join(format!(
            "radio-fm-config-limit-test-{}-{}",
            process::id(),
            CONFIG_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("radio-rust.json");
        fs::write(&path, vec![b' '; MAX_CONFIG_FILE_BYTES + 1]).expect("write oversized config");

        assert!(load_app_config(&path).is_err());

        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn restricts_permissions_when_loading_an_existing_configuration() {
        let directory = std::env::temp_dir().join(format!(
            "radio-fm-config-permissions-test-{}-{}",
            process::id(),
            CONFIG_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("radio-rust.json");
        fs::write(&path, "{}").expect("write config");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("loosen config permissions");

        load_app_config(&path).expect("load config");

        assert_eq!(
            fs::metadata(&path).expect("read metadata").mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
