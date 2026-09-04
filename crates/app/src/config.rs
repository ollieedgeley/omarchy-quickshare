//! User-visible endpoint settings stored as strict TOML.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Default outbound search window.
pub const DEFAULT_DISCOVERY_TIMEOUT_SECS: u64 = 15;
/// Default inbound visibility window.
pub const DEFAULT_VISIBILITY_TIMEOUT_SECS: u64 = 300;
/// Default active-transfer deadline.
pub const DEFAULT_TRANSFER_TIMEOUT_SECS: u64 = 120;

/// Strict local settings for the endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Outbound search deadline in seconds.
    pub discovery_timeout_secs: u64,
    /// Preferred peer identifier persisted across restarts.
    pub pinned_peer_id: Option<String>,
    /// Directory that receives completed inbound files.
    pub receive_directory: PathBuf,
    /// Active-transfer deadline in seconds.
    pub transfer_timeout_secs: u64,
    /// Inbound visibility window in seconds.
    pub visibility_timeout_secs: u64,
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            discovery_timeout_secs: DEFAULT_DISCOVERY_TIMEOUT_SECS,
            pinned_peer_id: None,
            receive_directory: default_receive_directory(),
            transfer_timeout_secs: DEFAULT_TRANSFER_TIMEOUT_SECS,
            visibility_timeout_secs: DEFAULT_VISIBILITY_TIMEOUT_SECS,
        }
    }
}

impl Config {
    /// Loads settings from the user config path, or defaults when absent.
    ///
    /// # Errors
    ///
    /// Returns an error when the file exists but is not strict TOML.
    pub fn load() -> io::Result<Self> {
        load_from(&config_path()?)
    }

    /// Writes these settings as strict TOML.
    ///
    /// # Errors
    ///
    /// Returns an error when the config directory or file cannot be written.
    pub fn save(&self) -> io::Result<()> {
        let path = config_path()?;
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        fs::write(path, self.to_toml())
    }

    /// Renders the effective settings as strict TOML.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut body = format!(
            "discovery_timeout_secs = {}\nreceive_directory = \"{}\"\n\
             transfer_timeout_secs = {}\nvisibility_timeout_secs = {}\n",
            self.discovery_timeout_secs,
            escape_toml(&self.receive_directory.display().to_string()),
            self.transfer_timeout_secs,
            self.visibility_timeout_secs,
        );
        if let Some(peer_id) = &self.pinned_peer_id {
            body.push_str(&format!(
                "pinned_peer_id = \"{}\"\n",
                escape_toml(peer_id)
            ));
        }
        body
    }

    /// Updates one documented setting and persists the file.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown key, invalid value, or write failure.
    pub fn set(&mut self, key: &str, value: &str) -> io::Result<()> {
        match key {
            "discovery_timeout_secs" => {
                self.discovery_timeout_secs = parse_timeout(value)?;
            }
            "pinned_peer_id" => {
                self.pinned_peer_id = non_empty(value);
            }
            "receive_directory" => {
                self.receive_directory = expand_user(Path::new(value));
            }
            "transfer_timeout_secs" => {
                self.transfer_timeout_secs = parse_timeout(value)?;
            }
            "visibility_timeout_secs" => {
                self.visibility_timeout_secs = parse_timeout(value)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "unknown config key '{key}'; expected \
                         receive_directory, pinned_peer_id, \
                         discovery_timeout_secs, visibility_timeout_secs, or \
                         transfer_timeout_secs"
                    ),
                ));
            }
        }
        self.save()
    }
}

/// Returns the default inbound destination under the user's downloads
/// directory.
///
/// Prefers a non-empty `$XDG_DOWNLOAD_DIR` and otherwise uses `~/Downloads`.
#[must_use]
pub fn default_receive_directory() -> PathBuf {
    env::var_os("XDG_DOWNLOAD_DIR")
        .filter(|value| !value.is_empty())
        .map(|root| PathBuf::from(root).join("omarchy-quickshare"))
        .unwrap_or_else(|| {
            expand_user(Path::new("~/Downloads/omarchy-quickshare"))
        })
}

/// Resolves `$XDG_CONFIG_HOME/omarchy-quickshare/config.toml`.
///
/// # Errors
///
/// Returns an error when neither `XDG_CONFIG_HOME` nor `HOME` is set.
pub fn config_path() -> io::Result<PathBuf> {
    let root = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
        })
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_CONFIG_HOME and HOME are unavailable; cannot locate \
                 config.toml",
            )
        })?;
    Ok(root.join("omarchy-quickshare/config.toml"))
}

/// Reads one strict config file, or defaults when it is missing.
fn load_from(path: &Path) -> io::Result<Config> {
    match fs::read_to_string(path) {
        Ok(body) => parse_toml(&body),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Config::default())
        }
        Err(error) => Err(error),
    }
}

/// Parses the documented key=value TOML subset and rejects unknown keys.
fn parse_toml(body: &str) -> io::Result<Config> {
    let mut config = Config::default();
    for (index, raw) in body.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(invalid_config(index, line));
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "discovery_timeout_secs" => {
                config.discovery_timeout_secs = parse_timeout(value)?;
            }
            "pinned_peer_id" => {
                config.pinned_peer_id = non_empty(&parse_string(value)?);
            }
            "receive_directory" => {
                config.receive_directory =
                    expand_user(Path::new(&parse_string(value)?));
            }
            "transfer_timeout_secs" => {
                config.transfer_timeout_secs = parse_timeout(value)?;
            }
            "visibility_timeout_secs" => {
                config.visibility_timeout_secs = parse_timeout(value)?;
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "invalid config.toml: unknown key '{key}'; unknown \
                         keys are rejected and every setting must use the \
                         documented names"
                    ),
                ));
            }
        }
    }
    Ok(config)
}

/// Expands a leading `~` using `$HOME`.
fn expand_user(path: &Path) -> PathBuf {
    let Some(home) = env::var_os("HOME").map(PathBuf::from) else {
        return path.to_path_buf();
    };
    let raw = path.to_string_lossy();
    if raw == "~" {
        return home;
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    path.to_path_buf()
}

/// Parses a non-negative timeout in seconds.
fn parse_timeout(value: &str) -> io::Result<u64> {
    value.parse::<u64>().map_err(|_error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "invalid timeout '{value}'; expected a non-negative integer"
            ),
        )
    })
}

/// Parses a quoted or bare TOML string.
fn parse_string(value: &str) -> io::Result<String> {
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return Ok(inner.replace("\\\"", "\"").replace("\\\\", "\\"));
    }
    if value.contains(' ') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config.toml string '{value}'"),
        ));
    }
    Ok(String::from(value))
}

/// Treats an empty string as an unset optional value.
fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(String::from(trimmed))
    }
}

/// Escapes a TOML basic string.
fn escape_toml(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Converts a malformed line into an actionable I/O error.
fn invalid_config(index: usize, line: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "invalid config.toml on line {}: '{line}'; unknown keys are \
             rejected and every setting must use the documented names",
            index.saturating_add(1)
        ),
    )
}
