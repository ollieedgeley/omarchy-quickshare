//! Control socket, config, timeout, and archive lifecycle for the daemon.

use core::time::Duration;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Instant;

use quickshare_control::request::Request;
use quickshare_sharing::{Attachment, DiscoveryState, Phase};
use quickshare_storage::OutboundSource;

use super::Daemon;
use super::network::NetworkWorker;
use super::notify::{self, NotifyKind};
use super::observations::{
    io_error_kind, recovery_guidance, trace_protocol, trace_storage,
};
use crate::archive;
use crate::config::Config;

/// Settings retained for already admitted operations.
#[derive(Debug, Default)]
pub(super) struct OperationTimeouts {
    discovery: Duration,
    transfer: Duration,
    transfer_share_id: Option<u64>,
}

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
    /// Applies validated settings without replacing active share state.
    pub(super) fn install_config(&mut self, config: Config) {
        self.preferences.saved = Some(super::preferences::values(&config));
        self.preferences_configured(config, None);
        if let Some(generation) =
            self.visibility.set_policy(self.config.discoverable)
        {
            self.activate_visibility(generation);
        }
        self.sync_visibility();
    }

    /// Pins a newly observed peer when it matches persisted preference.
    pub(super) fn pin_if_configured(&mut self, peer_id: &str) {
        if self.config.pinned_peer_id.as_deref() == Some(peer_id) {
            let _pinned = self.sharing.pin_peer(peer_id);
        }
    }

    /// Ends searches, visibility windows, and transfers that exceeded config.
    pub(super) fn apply_timeouts(&mut self) -> io::Result<()> {
        self.capture_timeout_settings();
        self.timeout_discovery()?;
        self.timeout_transfer()?;
        self.sync_visibility();
        Ok(())
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
            let attachment = Attachment::file(
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
        let attachment =
            Attachment::file(&source.name().to_string_lossy(), source.len());
        let share_id = self.queue_attachment_for(attachment, peer_id.is_some());
        self.outbound.remember_file(share_id, source);
        Ok(share_id)
    }

    fn timeout_discovery(&mut self) -> io::Result<()> {
        if self.sharing.snapshot().discovery() != DiscoveryState::Searching {
            self.discovery_started_at = None;
            return Ok(());
        }
        let started =
            *self.discovery_started_at.get_or_insert_with(Instant::now);
        if Instant::now().saturating_duration_since(started)
            >= self.timeouts.discovery
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
            < self.timeouts.transfer
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

    pub(super) fn capture_timeout_settings(&mut self) {
        let snapshot = self.sharing.snapshot();
        if snapshot.discovery() == DiscoveryState::Searching {
            if self.discovery_started_at.is_none() {
                self.discovery_started_at = Some(Instant::now());
                self.timeouts.discovery =
                    Duration::from_secs(self.config.discovery_timeout_secs);
            }
        } else {
            self.discovery_started_at = None;
        }
        if let Some(share) = snapshot.active_share()
            && matches!(
                share.phase(),
                Phase::WaitingForPeer
                    | Phase::AwaitingLocalConsent
                    | Phase::AwaitingPeerConsent
                    | Phase::Transferring
            )
        {
            if self.timeouts.transfer_share_id != Some(share.id().get()) {
                self.transfer_started_at = None;
                self.timeouts.transfer =
                    Duration::from_secs(self.config.transfer_timeout_secs);
                self.timeouts.transfer_share_id = Some(share.id().get());
            }
            if share.phase() == Phase::Transferring {
                let _started =
                    self.transfer_started_at.get_or_insert_with(Instant::now);
            } else {
                self.transfer_started_at = None;
            }
        } else {
            self.transfer_started_at = None;
            self.timeouts.transfer_share_id = None;
        }
    }
    /// Applies one simulator request and reports whether state changed.
    #[expect(
        clippy::pattern_type_mismatch,
        clippy::wildcard_enum_match_arm,
        reason = "Borrowed non-exhaustive requests require an unhandled case"
    )]
    pub(super) fn simulation_applied(&mut self, request: &Request) -> bool {
        match request {
            Request::SimulateDiscoveryTimeout => {
                self.sharing.discovery_timed_out()
            }
            Request::SimulateFail { share_id } => {
                let failed = self.sharing.fail(*share_id);
                if failed {
                    let reason = "simulated_failure";
                    let _observed = self.sharing.record_observation(
                        *share_id,
                        None,
                        None,
                        Some(reason),
                        Some(recovery_guidance(reason)),
                    );
                }
                failed
            }
            Request::SimulateIncomingFile { name, size_bytes } => {
                self.simulate_inbound(Attachment::file(name, *size_bytes))
            }
            Request::SimulateIncomingText { text } => {
                self.simulate_inbound(Attachment::text(text))
            }
            Request::SimulateIncomingUrl { url } => {
                self.simulate_inbound(Attachment::url(url))
            }
            Request::SimulatePeerAccept { share_id } => {
                self.sharing.accept_by_peer(*share_id)
            }
            Request::SimulatePeerLost { peer_id } => {
                self.sharing.remove_peer(peer_id)
            }
            Request::SimulatePeerReject { share_id } => {
                self.sharing.reject_by_peer(*share_id)
            }
            Request::SimulatePeerSeen { name, peer_id } => {
                self.sharing.observe_peer(peer_id, name);
                self.pin_if_configured(peer_id);
                true
            }
            Request::SimulateComplete { share_id } => {
                self.sharing.complete(*share_id)
            }
            Request::SimulateProgress {
                share_id,
                transferred_bytes,
            } => self.sharing.record_progress(*share_id, *transferred_bytes),
            _ => false,
        }
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
    let config_watch = super::preferences::ConfigWatch::new()?;
    let config = Config::load()?;
    fs::create_dir_all(&config.receive_directory)?;
    let socket = ControlSocket::bind(socket_path)?;
    let network = NetworkWorker::start(config.clone())?;
    let mut endpoint = Daemon::with_network_worker(network);
    endpoint.config_watch = Some(config_watch);
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
    let config_watch = super::preferences::ConfigWatch::new()?;
    let config = Config::load()?;
    let socket = ControlSocket::bind(socket_path)?;
    let mut endpoint = Daemon::simulated();
    endpoint.config_watch = Some(config_watch);
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
        daemon.capture_timeout_settings();
        daemon.transfer_started_at = Some(
            Instant::now()
                .checked_sub(Duration::from_secs(5))
                .expect("clock"),
        );
        daemon.apply_timeouts().expect("timeout applied");
        let share = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("failed share remains visible");
        assert_eq!(share.phase(), Phase::Failed);
        assert_eq!(share.terminal_reason(), Some("timed_out"));
    }
}
