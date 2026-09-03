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
    pub(super) fn apply_network_events(&mut self) -> io::Result<()> {
        while let Some(event) = self.next_network_event()? {
            self.apply_network_event(event);
        }
        Ok(())
    }

    /// Applies one production network observation to public daemon state.
    #[expect(
        clippy::print_stderr,
        reason = "Production transfer failures need actionable daemon logs"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "Exhaustive match keeps the network lifecycle together"
    )]
    fn apply_network_event(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::InboundCancelled { share_id } => {
                let _cancelled = self.sharing.cancel(share_id);
            }
            NetworkEvent::InboundCompleted { bytes, share_id } => {
                let _recorded = self.sharing.record_progress(share_id, bytes);
            }
            NetworkEvent::InboundFailed { reason, share_id } => {
                self.apply_inbound_failure(share_id, &reason);
            }
            NetworkEvent::InboundRejected { share_id } => {
                let _rejected = self.sharing.reject_inbound(share_id);
            }
            NetworkEvent::InboundOffered {
                name,
                size_bytes,
                verification_code,
            } => {
                self.sharing
                    .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
                if let Some(share_id) = self.sharing.offer_inbound(
                    quickshare_sharing::Attachment::file(&name, size_bytes),
                    INBOUND_PEER_ID,
                ) {
                    let _recorded = self.sharing.record_verification_code(
                        share_id.get(),
                        &verification_code,
                    );
                }
            }
            NetworkEvent::OutboundAccepted { share_id } => {
                let _accepted = self.sharing.accept_by_peer(share_id);
            }
            NetworkEvent::OutboundPairing {
                share_id,
                verification_code,
            } => {
                let _recorded = self
                    .sharing
                    .record_verification_code(share_id, &verification_code);
            }
            NetworkEvent::OutboundCancelled { share_id } => {
                let _cancelled = self.sharing.cancel(share_id);
                self.outbound.finish(share_id);
            }
            NetworkEvent::OutboundCompleted { bytes, share_id } => {
                let _recorded = self.sharing.record_progress(share_id, bytes);
                self.outbound.finish(share_id);
            }
            NetworkEvent::OutboundFailed { reason, share_id } => {
                eprintln!("outbound share {share_id} failed: {reason}");
                let _failed = self.sharing.fail(share_id);
                self.outbound.finish(share_id);
            }
            NetworkEvent::OutboundRejected { share_id } => {
                let _rejected = self.sharing.reject_by_peer(share_id);
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::inline_modules,
    reason = "Focused unit tests stay beside private event transitions"
)]
mod tests {
    use quickshare_sharing::{Attachment, Phase};

    use super::{Daemon, INBOUND_PEER_ID, INBOUND_PEER_NAME, NetworkEvent};

    fn active_phase(daemon: &Daemon) -> Phase {
        daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("share remains visible")
            .phase()
    }

    #[test]
    fn inbound_offer_exposes_pin_until_local_consent() {
        let mut daemon = Daemon::new();
        daemon.apply_network_event(NetworkEvent::InboundOffered {
            name: String::from("note.txt"),
            size_bytes: 12,
            verification_code: String::from("6251"),
        });

        let share = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("inbound offer is active");
        assert_eq!(share.phase(), Phase::AwaitingLocalConsent);
        assert_eq!(share.verification_code(), Some("6251"));
        let share_id = share.id().get();

        assert!(daemon.sharing.accept_inbound(share_id));
        let accepted = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("accepted inbound share remains active");
        assert_eq!(accepted.phase(), Phase::Transferring);
        assert_eq!(accepted.verification_code(), None);
    }

    #[test]
    fn inbound_terminal_events_preserve_rejection_and_cancellation() {
        let mut inbound = Daemon::new();
        inbound.apply_network_event(NetworkEvent::InboundOffered {
            name: String::from("note.txt"),
            size_bytes: 12,
            verification_code: String::from("6251"),
        });
        let inbound_id = inbound
            .sharing
            .snapshot()
            .active_share()
            .expect("inbound offer")
            .id()
            .get();
        inbound.apply_network_event(NetworkEvent::InboundRejected {
            share_id: inbound_id,
        });
        assert_eq!(active_phase(&inbound), Phase::Rejected);

        let mut cancelled = Daemon::new();
        cancelled.apply_network_event(NetworkEvent::InboundOffered {
            name: String::from("cancelled.txt"),
            size_bytes: 12,
            verification_code: String::from("9418"),
        });
        let cancelled_id = cancelled
            .sharing
            .snapshot()
            .active_share()
            .expect("inbound offer")
            .id()
            .get();
        assert!(cancelled.sharing.accept_inbound(cancelled_id));
        cancelled.apply_network_event(NetworkEvent::InboundCancelled {
            share_id: cancelled_id,
        });
        assert_eq!(active_phase(&cancelled), Phase::Cancelled);
    }

    #[test]
    fn outbound_terminal_events_preserve_rejection_and_cancellation() {
        let mut daemon = Daemon::new();
        daemon
            .sharing
            .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
        let rejected_id = daemon
            .sharing
            .queue_outbound(Attachment::file("note.txt", 12));
        assert!(
            daemon
                .sharing
                .select_peer(rejected_id.get(), INBOUND_PEER_ID)
        );
        daemon.apply_network_event(NetworkEvent::OutboundRejected {
            share_id: rejected_id.get(),
        });
        assert_eq!(active_phase(&daemon), Phase::Rejected);

        assert!(daemon.sharing.dismiss(rejected_id.get()));
        let cancelled_id = daemon
            .sharing
            .queue_outbound(Attachment::file("cancelled.txt", 12));
        assert!(
            daemon
                .sharing
                .select_peer(cancelled_id.get(), INBOUND_PEER_ID)
        );
        daemon.apply_network_event(NetworkEvent::OutboundAccepted {
            share_id: cancelled_id.get(),
        });
        daemon.apply_network_event(NetworkEvent::OutboundCancelled {
            share_id: cancelled_id.get(),
        });
        assert_eq!(active_phase(&daemon), Phase::Cancelled);
    }

    #[test]
    fn outbound_events_expose_pin_and_start_transfer_on_peer_acceptance() {
        let mut daemon = Daemon::new();
        daemon
            .sharing
            .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
        let share_id = daemon
            .sharing
            .queue_outbound(Attachment::file("note.txt", 12));
        assert!(daemon.sharing.select_peer(share_id.get(), INBOUND_PEER_ID));

        daemon.apply_network_event(NetworkEvent::OutboundPairing {
            share_id: share_id.get(),
            verification_code: String::from("9418"),
        });
        let pairing = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("outbound share awaits peer");
        assert_eq!(pairing.phase(), Phase::AwaitingPeerConsent);
        assert_eq!(pairing.verification_code(), Some("9418"));

        daemon.apply_network_event(NetworkEvent::OutboundAccepted {
            share_id: share_id.get(),
        });
        let transferring = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("accepted outbound share remains active");
        assert_eq!(transferring.phase(), Phase::Transferring);
        assert_eq!(transferring.verification_code(), None);

        daemon.apply_network_event(NetworkEvent::OutboundCompleted {
            bytes: 12,
            share_id: share_id.get(),
        });
        let completed = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("completed outbound share remains visible");
        assert_eq!(completed.phase(), Phase::Completed);
    }
}
