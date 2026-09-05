//! Control socket, config, timeout, and archive lifecycle for the daemon.

use core::time::Duration;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Instant;

use quickshare_sharing::{
    DiscoveryState, PeerSnapshot, Phase, VisibilityState,
};
use quickshare_storage::OutboundSource;

use super::Daemon;
use super::network::NetworkWorker;
use super::notify::{self, NotifyKind};
use super::observations::{
    io_error_kind, recovery_guidance, trace_protocol, trace_storage,
};
use crate::archive;
use crate::config::Config;

fn open_source(path: &Path) -> io::Result<OutboundSource> {
    let source = OutboundSource::open(path).map_err(|error| {
        trace_storage("open_source", "failed", Some(&error));
        io::Error::other(error)
    })?;
    trace_storage("open_source", "completed", None);
    Ok(source)
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Config and timeout behavior stay out of the control-loop file"
)]
impl Daemon {
    /// Applies persisted settings to a newly constructed endpoint.
    pub(super) fn install_config(&mut self, config: Config) {
        if let Some(peer_id) = config.pinned_peer_id.as_deref() {
            let _pinned = self.sharing.pin_peer(peer_id);
        }
        self.config = config;
    }

    /// Pins a newly observed peer when it matches persisted preference.
    pub(super) fn pin_if_configured(&mut self, peer_id: &str) {
        if self.config.pinned_peer_id.as_deref() == Some(peer_id) {
            let _pinned = self.sharing.pin_peer(peer_id);
        }
    }

    /// Ends searches, visibility windows, and transfers that exceeded config.
    pub(super) fn apply_timeouts(&mut self) -> io::Result<()> {
        self.timeout_discovery()?;
        self.timeout_transfer()?;
        self.timeout_visibility()
    }

    /// Queues a file or a ZIP of a folder and remembers temporary archives.
    pub(super) fn queue_file(
        &mut self,
        path: &Path,
        peer_id: Option<&str>,
    ) -> io::Result<u64> {
        if path.is_dir() {
            let archive_path =
                archive::zip_directory(path).inspect_err(|error| {
                    trace_protocol(
                        "source",
                        "archive",
                        "failed",
                        Some("io"),
                        Some(io_error_kind(error)),
                    );
                })?;
            trace_protocol("source", "archive", "completed", None, None);
            let source = open_source(&archive_path)?;
            let attachment = quickshare_sharing::Attachment::file(
                &source.name().to_string_lossy(),
                source.len(),
            );
            let share_id =
                self.queue_attachment_for(attachment, peer_id.is_some());
            self.outbound.remember_file(share_id, source);
            self.outbound.remember_archive(share_id, archive_path);
            return Ok(share_id);
        }
        let source = open_source(path)?;
        let attachment = quickshare_sharing::Attachment::file(
            &source.name().to_string_lossy(),
            source.len(),
        );
        let share_id = self.queue_attachment_for(attachment, peer_id.is_some());
        self.outbound.remember_file(share_id, source);
        Ok(share_id)
    }

    pub(super) fn unpin_peers(&mut self) -> io::Result<bool> {
        let had_pin = self
            .sharing
            .snapshot()
            .peers()
            .iter()
            .any(PeerSnapshot::is_pinned)
            || self.config.pinned_peer_id.is_some();
        self.sharing.unpin_peers();
        self.config.pinned_peer_id = None;
        self.config.save()?;
        Ok(had_pin)
    }

    /// Persists the single preferred peer after a successful live pin.
    pub(super) fn persist_pin(&mut self, peer_id: &str) -> io::Result<()> {
        self.config.pinned_peer_id = Some(String::from(peer_id));
        self.config.save()
    }

    fn timeout_discovery(&mut self) -> io::Result<()> {
        if self.sharing.snapshot().discovery() != DiscoveryState::Searching {
            self.discovery_started_at = None;
            return Ok(());
        }
        let started =
            *self.discovery_started_at.get_or_insert_with(Instant::now);
        if Instant::now().saturating_duration_since(started)
            >= Duration::from_secs(self.config.discovery_timeout_secs)
        {
            let _timed_out = self.sharing.discovery_timed_out();
            tracing::info!(
                target: "omarchy_quickshare::protocol",
                stage = "discovery",
                operation = "deadline",
                outcome = "timed_out",
                phase = "timed_out",
                "discovery timed out"
            );
            if let Some(network) = &self.network {
                network.stop_discovery()?;
            }
            self.discovery_started_at = None;
        }
        Ok(())
    }

    fn timeout_visibility(&mut self) -> io::Result<()> {
        if self.sharing.snapshot().visibility() != VisibilityState::Open {
            self.visibility_opened_at = None;
            return Ok(());
        }
        let started =
            *self.visibility_opened_at.get_or_insert_with(Instant::now);
        if Instant::now().saturating_duration_since(started)
            < Duration::from_secs(self.config.visibility_timeout_secs)
        {
            return Ok(());
        }
        self.sharing.close_visibility();
        tracing::info!(
            target: "omarchy_quickshare::protocol",
            stage = "visibility",
            operation = "deadline",
            outcome = "completed",
            reason = "timed_out",
            phase = "closed",
            "visibility closed"
        );
        if let Some(network) = &self.network {
            network.close_visibility()?;
        }
        self.visibility_opened_at = None;
        Ok(())
    }

    fn timeout_transfer(&mut self) -> io::Result<()> {
        let Some(active) = self.sharing.snapshot().active_share() else {
            self.transfer_started_at = None;
            return Ok(());
        };
        if active.phase() != Phase::Transferring {
            self.transfer_started_at = None;
            return Ok(());
        }
        let share_id = active.id().get();
        let started =
            *self.transfer_started_at.get_or_insert_with(Instant::now);
        if Instant::now().saturating_duration_since(started)
            < Duration::from_secs(self.config.transfer_timeout_secs)
        {
            return Ok(());
        }
        if self.sharing.fail(share_id) {
            tracing::warn!(
                target: "omarchy_quickshare::protocol",
                share_id,
                stage = "transfer",
                operation = "deadline",
                outcome = "failed",
                reason = "timed_out",
                phase = "failed",
                error_class = "timed_out",
                "share failed"
            );
            let _observed = self.sharing.record_observation(
                share_id,
                None,
                None,
                Some("timed_out"),
                Some(recovery_guidance("timed_out")),
            );
            if let Some(network) = &self.network {
                network.cancel_transfer(share_id);
            }
            notify::notify(NotifyKind::Error);
        }
        self.outbound.finish(share_id);
        self.transfer_started_at = None;
        Ok(())
    }
}

/// Owner-only mode for the control socket directory.
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
/// Owner-only mode for the control socket.
const PRIVATE_SOCKET_MODE: u32 = 0o600;

/// A bound control listener that removes its socket on a clean shutdown.
#[derive(Debug)]
struct ControlSocket {
    /// The listener served by the local endpoint.
    listener: UnixListener,
    /// The filesystem entry removed when the listener is dropped.
    path: PathBuf,
}

impl ControlSocket {
    /// Binds an owner-only socket after rejecting a running endpoint.
    fn bind(path: &Path) -> io::Result<Self> {
        let directory = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "control socket has no parent directory",
            )
        })?;
        fs::create_dir_all(directory)?;
        fs::set_permissions(
            directory,
            fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
        )?;
        remove_stale_socket(path)?;
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(PRIVATE_SOCKET_MODE),
        )?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for ControlSocket {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.path));
    }
}

/// Removes an abandoned socket without replacing a running endpoint.
#[expect(
    clippy::single_call_fn,
    reason = "The stale-socket decision remains separate from binding"
)]
fn remove_stale_socket(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match UnixStream::connect(path) {
        Ok(_stream) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "local endpoint is already running",
        )),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            fs::remove_file(path)
        }
        Err(error) => Err(error),
    }
}

/// Runs the local endpoint until the process is terminated.
///
/// # Errors
pub fn run(socket_path: &Path) -> io::Result<()> {
    let config = Config::load()?;
    fs::create_dir_all(&config.receive_directory)?;
    let socket = ControlSocket::bind(socket_path)?;
    let network = NetworkWorker::start(
        config.receive_directory.clone(),
        Duration::from_secs(config.visibility_timeout_secs),
    )?;
    let mut endpoint = Daemon::with_network_worker(network);
    endpoint.install_config(config);
    tracing::info!(phase = "ready", "daemon ready");
    let result = endpoint.serve_until(&socket.listener, || false);
    tracing::info!(phase = "shutdown", "daemon shutdown");
    result
}

/// Runs a deterministic local peer for complete application testing.
///
/// # Errors
///
/// Returns an error when the private control socket cannot be served.
#[inline]
pub fn run_simulated(socket_path: &Path) -> io::Result<()> {
    let config = Config::load()?;
    let socket = ControlSocket::bind(socket_path)?;
    let mut endpoint = Daemon::simulated();
    endpoint.install_config(config);
    endpoint.serve_until(&socket.listener, || false)
}

#[cfg(test)]
mod tests {
    use super::super::Daemon;
    use core::time::Duration;
    use quickshare_sharing::{Attachment, Phase};
    use std::time::Instant;

    #[test]
    fn transfer_timeout_records_reason_and_guidance() {
        let mut daemon = Daemon::new();
        daemon.config.transfer_timeout_secs = 1;
        daemon.sharing.observe_peer("peer", "Peer");
        let share_id = daemon.sharing.queue_outbound(Attachment::text("hi"));
        assert!(daemon.sharing.select_peer(share_id.get(), "peer"));
        assert!(daemon.sharing.accept_by_peer(share_id.get()));
        daemon.transfer_started_at = Some(
            Instant::now()
                .checked_sub(Duration::from_secs(5))
                .expect("clock"),
        );
        daemon.timeout_transfer().expect("timeout applied");
        let share = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("failed share remains visible");
        assert_eq!(share.phase(), Phase::Failed);
        assert_eq!(share.terminal_reason(), Some("timed_out"));
        assert_eq!(
            share.recovery_guidance(),
            Some("Retry while both devices stay nearby.")
        );
    }
}
