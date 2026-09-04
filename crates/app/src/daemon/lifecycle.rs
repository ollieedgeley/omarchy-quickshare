//! Config, timeouts, pin persistence, and folder archives for the daemon.

use core::time::Duration;
use std::io;
use std::time::Instant;

use quickshare_sharing::{
    DiscoveryState, PeerSnapshot, Phase, VisibilityState,
};
use quickshare_storage::OutboundSource;

use super::Daemon;
use super::notify::{self, NotifyKind};
use super::observations::recovery_guidance;
use crate::archive;
use crate::config::Config;

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
        path: &std::path::Path,
    ) -> io::Result<u64> {
        if path.is_dir() {
            let archive_path = archive::zip_directory(path)?;
            let source = OutboundSource::open(&archive_path)
                .map_err(io::Error::other)?;
            let attachment = quickshare_sharing::Attachment::file(
                &source.name().to_string_lossy(),
                source.len(),
            );
            let share_id = self.queue_attachment(attachment);
            self.outbound.remember_file(share_id, source);
            self.outbound.remember_archive(share_id, archive_path);
            let _started = self.start_pinned_outbound(share_id);
            return Ok(share_id);
        }
        let source = OutboundSource::open(path).map_err(io::Error::other)?;
        let attachment = quickshare_sharing::Attachment::file(
            &source.name().to_string_lossy(),
            source.len(),
        );
        let share_id = self.queue_attachment(attachment);
        self.outbound.remember_file(share_id, source);
        let _started = self.start_pinned_outbound(share_id);
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
