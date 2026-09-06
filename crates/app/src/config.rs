//! User-visible endpoint settings stored as strict TOML.

use core::sync::atomic::{AtomicU64, Ordering};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process;

use toml_edit::{DocumentMut, Item, Value};

/// Default outbound search window.
pub const DEFAULT_DISCOVERY_TIMEOUT_SECS: u64 = 15;
/// Default pending inbound consent deadline.
pub const DEFAULT_CONSENT_TIMEOUT_SECS: u64 = 300;
/// Default active-transfer deadline.
pub const DEFAULT_TRANSFER_TIMEOUT_SECS: u64 = 120;

/// Distinguishes temporary files across writes and skips interrupted writes.
static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(0);

/// Strict local settings for the endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Pending inbound consent deadline captured when an offer arrives.
    pub consent_timeout_secs: u64,
    /// Optional device name advertised instead of the system hostname.
    pub device_name: Option<String>,
    /// Whether saved policy allows inbound discovery.
    pub discoverable: bool,
    /// Outbound search deadline in seconds.
    pub discovery_timeout_secs: u64,
    /// Preferred peer identifier persisted across restarts.
    pub pinned_peer_id: Option<String>,
    /// Whether the future picker may read the clipboard when selecting.
    pub read_clipboard_on_select: bool,
    /// Directory that receives completed inbound files.
    pub receive_directory: PathBuf,
    /// Active-transfer deadline in seconds.
    pub transfer_timeout_secs: u64,
}

impl Default for Config {
    #[inline]
    fn default() -> Self {
        Self {
            consent_timeout_secs: DEFAULT_CONSENT_TIMEOUT_SECS,
            device_name: None,
            discoverable: false,
            discovery_timeout_secs: DEFAULT_DISCOVERY_TIMEOUT_SECS,
            pinned_peer_id: None,
            read_clipboard_on_select: false,
            receive_directory: default_receive_directory(),
            transfer_timeout_secs: DEFAULT_TRANSFER_TIMEOUT_SECS,
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

    /// Renders the effective settings as strict TOML.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut body = format!(
            "consent_timeout_secs = {}\ndiscoverable = {}\n\
             read_clipboard_on_select = {}\n\
             discovery_timeout_secs = {}\nreceive_directory = {}\n\
             transfer_timeout_secs = {}\n",
            self.consent_timeout_secs,
            self.discoverable,
            self.read_clipboard_on_select,
            self.discovery_timeout_secs,
            Value::from(self.receive_directory.display().to_string()),
            self.transfer_timeout_secs,
        );
        if let Some(device_name) = &self.device_name {
            body.push_str(&format!(
                "device_name = {}\n",
                Value::from(device_name.as_str())
            ));
        }
        if let Some(peer_id) = &self.pinned_peer_id {
            body.push_str(&format!(
                "pinned_peer_id = {}\n",
                Value::from(peer_id.as_str())
            ));
        }
        body
    }

    /// Validates and atomically commits one key against the latest file.
    ///
    /// Product writers serialize through a private sibling lock. Editors
    /// need not lock, but an edit detected before replacement aborts the patch.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid current settings, an invalid patch, a
    /// concurrent external edit, or a failure before atomic replacement.
    pub fn patch(key: &str, value: &str) -> io::Result<Self> {
        let path = config_path()?;
        let directory = path
            .parent()
            .ok_or_else(|| io::Error::other("config parent unavailable"))?;
        fs::create_dir_all(directory)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(directory.join(".config.lock"))?;
        lock.lock()?;
        let original = read_document(&path)?;
        let mut document = parse_document(original.as_deref().unwrap_or(""))?;
        let current = from_document(&document)?;
        if !document.contains_key("consent_timeout_secs") {
            let consent = i64::try_from(current.consent_timeout_secs).map_err(
                |error| io::Error::new(io::ErrorKind::InvalidData, error),
            )?;
            document["consent_timeout_secs"] =
                Item::Value(Value::from(consent));
        }
        let mut replacement = patch_value(key, value)?;
        if let Some(previous) = document.get(key).and_then(Item::as_value) {
            *replacement.decor_mut() = previous.decor().clone();
        }
        document[key] = Item::Value(replacement);
        let candidate = from_document(&document)?;
        let (temporary, mut file) = loop {
            let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
            let temporary = directory
                .join(format!(".config-{}-{sequence}.tmp", process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };
        let result = (|| {
            file.write_all(document.to_string().as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            if read_document(&path)? != original {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "config.toml changed during the patch; \
                     retry against the current settings",
                ));
            }
            fs::rename(&temporary, &path)
        })();
        if result.is_err() {
            let _cleanup = fs::remove_file(&temporary);
        }
        result?;
        Ok(candidate)
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
    let document =
        parse_document(read_document(path)?.as_deref().unwrap_or(""))?;
    from_document(&document)
}

/// Reads the bytes used both for parsing and detecting external changes.
fn read_document(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(body) => Ok(Some(body)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Parses real TOML while retaining the original formatting.
fn parse_document(body: &str) -> io::Result<DocumentMut> {
    body.parse::<DocumentMut>().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid config.toml: {error}"),
        )
    })
}

/// Validates every setting without accepting undocumented keys.
fn from_document(document: &DocumentMut) -> io::Result<Config> {
    let mut config = Config::default();
    for (key, item) in document.iter() {
        let invalid = || {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid config.toml value for '{key}'"),
            )
        };
        match key {
            "device_name" => {
                config.device_name =
                    parse_device_name(item.as_str().ok_or_else(invalid)?)?;
            }
            "pinned_peer_id" => {
                config.pinned_peer_id =
                    non_empty(item.as_str().ok_or_else(invalid)?);
            }
            "receive_directory" => {
                let value = item.as_str().ok_or_else(invalid)?;
                if value.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "receive_directory must not be empty",
                    ));
                }
                config.receive_directory = expand_user(Path::new(value));
            }
            "discoverable" => {
                config.discoverable = item.as_bool().ok_or_else(invalid)?;
            }
            "read_clipboard_on_select" => {
                config.read_clipboard_on_select =
                    item.as_bool().ok_or_else(invalid)?;
            }
            "consent_timeout_secs"
            | "discovery_timeout_secs"
            | "visibility_timeout_secs"
            | "transfer_timeout_secs" => {
                let timeout =
                    u64::try_from(item.as_integer().ok_or_else(invalid)?)
                        .map_err(|_error| invalid())?;
                match key {
                    "consent_timeout_secs" => {
                        config.consent_timeout_secs = timeout;
                    }
                    "discovery_timeout_secs" => {
                        config.discovery_timeout_secs = timeout;
                    }
                    "visibility_timeout_secs" => {
                        if !document.contains_key("consent_timeout_secs") {
                            config.consent_timeout_secs = timeout;
                        }
                    }
                    _ => config.transfer_timeout_secs = timeout,
                }
            }
            _ => return Err(unknown_key(key)),
        }
    }
    Ok(config)
}

/// Converts the CLI/control string value to the setting's TOML type.
fn patch_value(key: &str, value: &str) -> io::Result<Value> {
    match key {
        "device_name" => {
            Ok(Value::from(parse_device_name(value)?.unwrap_or_default()))
        }
        "pinned_peer_id" => {
            Ok(Value::from(non_empty(value).unwrap_or_default()))
        }
        "receive_directory" => Ok(Value::from(
            expand_user(Path::new(value)).display().to_string(),
        )),
        "discoverable" | "read_clipboard_on_select" => {
            value.parse::<bool>().map(Value::from).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidInput, error)
            })
        }
        "consent_timeout_secs"
        | "discovery_timeout_secs"
        | "transfer_timeout_secs" => {
            let timeout =
                i64::try_from(parse_timeout(value)?).map_err(|error| {
                    io::Error::new(io::ErrorKind::InvalidInput, error)
                })?;
            Ok(Value::from(timeout))
        }
        _ => Err(unknown_key(key)),
    }
}

fn unknown_key(key: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("unknown config key '{key}'"),
    )
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

/// Parses an optional endpoint name that fits the wire advertisement.
fn parse_device_name(value: &str) -> io::Result<Option<String>> {
    let name = non_empty(value);
    if name
        .as_ref()
        .is_some_and(|name| name.len() > u8::MAX.into())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "device_name must be at most 255 bytes",
        ));
    }
    Ok(name)
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
