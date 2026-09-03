//! Production network behavior for the local endpoint.

use std::io;

use super::Daemon;
use super::network::{NetworkEvent, NetworkWorker};

/// Stable placeholder identity until the inbound protocol exposes details.
const INBOUND_PEER_ID: &str = "inbound-peer";
/// User-facing name for the inbound sender until the protocol exposes one.
const INBOUND_PEER_NAME: &str = "Nearby sender";

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Network event processing follows the event lifecycle"
)]
impl Daemon {
    /// Applies queued worker observations at the local-control seam.
    #[expect(
        clippy::print_stderr,
        reason = "Production transfer failures need actionable daemon logs"
    )]
    pub(super) fn apply_network_events(&mut self) -> io::Result<()> {
        while let Some(event) = self.next_network_event()? {
            match event {
                NetworkEvent::InboundCompleted { bytes, share_id } => {
                    let _recorded =
                        self.sharing.record_progress(share_id, bytes);
                }
                NetworkEvent::InboundFailed { reason, share_id } => {
                    self.apply_inbound_failure(share_id, &reason);
                }
                NetworkEvent::InboundOffered { name, size_bytes } => {
                    self.sharing
                        .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
                    let _offered = self.sharing.offer_inbound(
                        quickshare_sharing::Attachment::file(&name, size_bytes),
                        INBOUND_PEER_ID,
                    );
                }
                NetworkEvent::OutboundCompleted { bytes, share_id } => {
                    let _accepted = self.sharing.accept_by_peer(share_id);
                    let _recorded =
                        self.sharing.record_progress(share_id, bytes);
                    self.outbound.finish(share_id);
                }
                NetworkEvent::OutboundFailed { reason, share_id } => {
                    eprintln!("outbound share {share_id} failed: {reason}");
                    let _failed = self.sharing.fail(share_id);
                    self.outbound.finish(share_id);
                }
                NetworkEvent::PeerSeen {
                    name,
                    peer_id,
                    route,
                } => {
                    self.sharing.observe_peer(&peer_id, &name);
                    self.outbound.remember_peer(&peer_id, route);
                }
            }
        }
        Ok(())
    }

    #[expect(
        clippy::print_stderr,
        reason = "Production transfer failures need actionable daemon logs"
    )]
    /// Records a failed inbound share after writing diagnostic evidence.
    fn apply_inbound_failure(
        &mut self,
        candidate_share_id: Option<u64>,
        reason: &str,
    ) {
        eprintln!("inbound share failed: {reason}");
        if let Some(share_id) = candidate_share_id {
            let _failed = self.sharing.fail(share_id);
        }
    }

    /// Polls one production event without retaining a borrow of the worker.
    fn next_network_event(&self) -> io::Result<Option<NetworkEvent>> {
        self.network
            .as_ref()
            .map_or(Ok(None), NetworkWorker::next_event)
    }

    /// Starts a real transfer after validating local state and private routing.
    pub(super) fn select_peer(&mut self, share_id: u64, peer_id: &str) -> bool {
        let Some(network) = self.network.as_ref() else {
            return self.sharing.select_peer(share_id, peer_id);
        };
        let Some(transfer) = self.outbound.transfer(share_id, peer_id) else {
            return false;
        };
        if !self.sharing.select_peer(share_id, peer_id) {
            return false;
        }
        if network.send_file(share_id, transfer).is_ok() {
            return true;
        }
        let _failed = self.sharing.fail(share_id);
        false
    }
}
