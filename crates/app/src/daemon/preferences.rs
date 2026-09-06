//! Coalesced directory notifications for editor replacement saves.

use core::mem::MaybeUninit;
use core::time::Duration;
use std::fs::DirBuilder;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::DirBuilderExt as _;
use std::time::Instant;

use quickshare_control::response::{
    Envelope as ResponseEnvelope, PreferenceValues,
};
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};

use crate::config::{Config, config_path};

/// Checks queued directory events without rereading an unchanged document.
#[derive(Debug)]
pub(super) struct ConfigWatch {
    descriptor: OwnedFd,
    next_poll: Instant,
}

impl ConfigWatch {
    pub(super) fn new() -> io::Result<Self> {
        let path = config_path()?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("config path has no parent"))?;
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let descriptor =
            inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        let _watch = inotify::add_watch(
            &descriptor,
            parent,
            WatchFlags::CLOSE_WRITE | WatchFlags::MOVED_TO | WatchFlags::DELETE,
        )?;
        Ok(Self {
            descriptor,
            next_poll: Instant::now(),
        })
    }

    fn changed(&mut self) -> io::Result<bool> {
        let now = Instant::now();
        if now < self.next_poll {
            return Ok(false);
        }
        self.next_poll = now + Duration::from_millis(100);
        let mut buffer = [MaybeUninit::uninit(); 4096];
        let mut reader = inotify::Reader::new(&self.descriptor, &mut buffer);
        let mut changed = false;
        loop {
            match reader.next() {
                Ok(event) => {
                    changed |= event
                        .file_name()
                        .is_some_and(|name| name.to_bytes() == b"config.toml")
                        || event.events().contains(ReadFlags::QUEUE_OVERFLOW);
                }
                Err(rustix::io::Errno::AGAIN) => return Ok(changed),
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Preference reload stays with its directory watcher"
)]
impl super::Daemon {
    pub(super) fn reload_preferences(&mut self) -> io::Result<()> {
        if let Some(watch) = &mut self.config_watch
            && watch.changed()?
        {
            match Config::load() {
                Ok(config) => {
                    if self.preferences.saved.as_ref() != Some(&values(&config))
                    {
                        self.configure_preferences(config);
                    }
                }
                Err(error) => {
                    self.preferences.saved = None;
                    self.preferences.pending = false;
                    self.preferences.error = Some(format!(
                        "Cannot load config.toml: {error}; \
                         fix the document and save it again"
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn patch_preferences(
        &mut self,
        key: &str,
        value: &str,
    ) -> ResponseEnvelope {
        match Config::patch(key, value) {
            Ok(config) => {
                if key == "discoverable" && !config.discoverable {
                    self.visibility.close();
                }
                self.configure_preferences(config);
            }
            Err(error) => {
                match Config::load() {
                    Ok(config) => {
                        if self.preferences.saved.as_ref()
                            != Some(&values(&config))
                        {
                            self.configure_preferences(config);
                        }
                    }
                    Err(_) => {
                        self.preferences.saved = None;
                        self.preferences.pending = false;
                    }
                }
                self.preferences.error =
                    Some(format!("Preference was not saved: {error}"));
            }
        }
        ResponseEnvelope::preferences(self.preferences.clone())
    }

    fn configure_preferences(&mut self, config: Config) {
        self.preferences.saved = Some(values(&config));
        self.preferences.error = None;
        let activation = self.visibility.set_policy(config.discoverable);
        self.sync_visibility();
        if let Some(network) = &self.network {
            self.preferences.pending = true;
            if let Err(error) = network.configure(config) {
                self.preferences.pending = false;
                self.preferences.error =
                    Some(format!("Preferences saved but unavailable: {error}"));
            }
        } else {
            let error = std::fs::create_dir_all(&config.receive_directory)
                .err()
                .map(|error| error.to_string());
            self.preferences_configured(config, error);
        }
        if let Some(generation) = activation {
            self.activate_visibility(generation);
        }
        self.sync_visibility();
    }

    pub(super) fn preferences_configured(
        &mut self,
        config: Config,
        error: Option<String>,
    ) {
        let applied = values(&config);
        let current = self.preferences.saved.as_ref() == Some(&applied);
        if let Some(error) = error {
            if current {
                self.preferences.pending = false;
                self.preferences.error =
                    Some(format!("Preferences saved but unavailable: {error}"));
            }
            return;
        }
        self.capture_timeout_settings();
        self.sharing.unpin_peers();
        if let Some(peer_id) = config.pinned_peer_id.as_deref() {
            let _pinned = self.sharing.pin_peer(peer_id);
        }
        self.config = config;
        self.preferences.applied = Some(applied);
        if current {
            self.preferences.pending = false;
            self.preferences.error = None;
        }
    }
}

/// Converts application settings into the public control representation.
pub(super) fn values(config: &Config) -> PreferenceValues {
    PreferenceValues {
        consent_timeout_secs: config.consent_timeout_secs,
        device_name: config.device_name.clone(),
        discoverable: config.discoverable,
        read_clipboard_on_select: config.read_clipboard_on_select,
        receive_directory: config.receive_directory.clone(),
        discovery_timeout_secs: config.discovery_timeout_secs,
        transfer_timeout_secs: config.transfer_timeout_secs,
        pinned_peer_id: config.pinned_peer_id.clone(),
    }
}
