//! Production network behavior for the local endpoint.

use std::io;
use std::time::Instant;

use quickshare_sharing::{Attachment, Direction, OfferKind, Phase};

use super::Daemon;
use super::media::medium_name;
use super::network::{NetworkEvent, NetworkWorker};
use super::notify::{self, NotifyKind};
use super::observations::{recovery_guidance, remaining_seconds};

const INBOUND_PEER_ID: &str = "inbound-peer";
const INBOUND_PEER_NAME: &str = "Nearby sender";

fn completion_notice(recorded: bool, phase: Phase) -> bool {
    recorded && phase == Phase::Completed
}

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
            NetworkEvent::InboundCompleted {
                bytes,
                kind,
                share_id,
                value,
            } => {
                if let Some(value) = value {
                    let attachment = match kind {
                        OfferKind::Url => Attachment::url(&value),
                        _ => Attachment::text(&value),
                    };
                    let _replaced =
                        self.sharing.replace_attachment(share_id, attachment);
                }
                let recorded = self.sharing.record_progress(share_id, bytes);
                self.notify_if_completed(recorded, NotifyKind::Received);
            }
            NetworkEvent::InboundFailed { reason, share_id } => {
                self.apply_inbound_failure(share_id, &reason);
            }
            NetworkEvent::InboundRejected { share_id } => {
                let _rejected = self.sharing.reject_inbound(share_id);
            }
            NetworkEvent::InboundOffered {
                kind,
                name,
                size_bytes,
                verification_code,
            } => {
                self.sharing
                    .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
                let attachment = match kind {
                    OfferKind::AndroidApp | OfferKind::File => {
                        Attachment::file(&name, size_bytes)
                    }
                    OfferKind::Text => Attachment::text(""),
                    OfferKind::Url => Attachment::url(""),
                };
                if let Some(share_id) = self.sharing.offer_inbound_sized(
                    attachment,
                    INBOUND_PEER_ID,
                    Some(size_bytes),
                ) {
                    let _recorded = self.sharing.record_verification_code(
                        share_id.get(),
                        &verification_code,
                    );
                }
            }
            NetworkEvent::OutboundAccepted { share_id } => {
                let _accepted = self.sharing.accept_by_peer(share_id);
                self.transfer_started_at = Some(Instant::now());
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
                let recorded = self.sharing.record_progress(share_id, bytes);
                self.outbound.finish(share_id);
                self.notify_if_completed(recorded, NotifyKind::Sent);
            }
            NetworkEvent::OutboundFailed { reason, share_id } => {
                eprintln!("outbound share {share_id} failed: {reason}");
                let _failed = self.sharing.fail(share_id);
                let _observed = self.sharing.record_observation(
                    share_id,
                    None,
                    None,
                    Some(&reason),
                    Some(recovery_guidance(&reason)),
                );
                self.outbound.finish(share_id);
                notify::notify(NotifyKind::Error);
            }
            NetworkEvent::OutboundRejected { share_id } => {
                let _rejected = self.sharing.reject_by_peer(share_id);
                self.outbound.finish(share_id);
            }
            NetworkEvent::PeerLost { peer_id } => {
                let _removed = self.sharing.remove_peer(&peer_id);
                self.outbound.forget_peer(&peer_id);
            }
            NetworkEvent::PeerSeen {
                name,
                peer_id,
                route,
            } => {
                self.sharing.observe_peer(&peer_id, &name);
                self.pin_if_configured(&peer_id);
                self.outbound.remember_peer(&peer_id, route);
                self.auto_start_pinned(&peer_id);
            }
            NetworkEvent::Progress {
                medium,
                share_id,
                transferred_bytes,
            } => {
                let _recorded =
                    self.sharing.record_progress(share_id, transferred_bytes);
                if self.transfer_started_at.is_none() {
                    self.transfer_started_at = Some(Instant::now());
                }
                let remaining = self.transfer_started_at.and_then(|started| {
                    remaining_seconds(
                        transferred_bytes,
                        self.sharing
                            .snapshot()
                            .active_share()
                            .map(|share| share.total_bytes())
                            .unwrap_or(transferred_bytes),
                        Instant::now().saturating_duration_since(started),
                    )
                });
                let _observed = self.sharing.record_observation(
                    share_id,
                    Some(&medium),
                    remaining,
                    None,
                    None,
                );
            }
        }
    }

    #[expect(
        clippy::print_stderr,
        reason = "Production transfer failures need actionable daemon logs"
    )]
    fn apply_inbound_failure(
        &mut self,
        candidate_share_id: Option<u64>,
        reason: &str,
    ) {
        eprintln!("inbound share failed: {reason}");
        if let Some(share_id) =
            candidate_share_id.or_else(|| self.active_inbound_consent_id())
        {
            let _failed = self.sharing.fail(share_id);
            let _observed = self.sharing.record_observation(
                share_id,
                None,
                None,
                Some(reason),
                Some(recovery_guidance(reason)),
            );
        }
        notify::notify(NotifyKind::Error);
    }

    fn active_inbound_consent_id(&self) -> Option<u64> {
        let share = self.sharing.snapshot().active_share()?;
        (share.direction() == Direction::Inbound
            && share.phase() == Phase::AwaitingLocalConsent)
            .then(|| share.id().get())
    }

    fn notify_if_completed(&self, recorded: bool, kind: NotifyKind) {
        if completion_notice(recorded, self.active_completed_phase()) {
            notify::notify(kind);
        }
    }

    fn active_completed_phase(&self) -> Phase {
        self.sharing
            .snapshot()
            .active_share()
            .map_or(Phase::Failed, |share| share.phase())
    }

    fn next_network_event(&self) -> io::Result<Option<NetworkEvent>> {
        self.network
            .as_ref()
            .map_or(Ok(None), NetworkWorker::next_event)
    }

    pub(super) fn select_peer(&mut self, share_id: u64, peer_id: &str) -> bool {
        let Some(network) = self.network.as_ref() else {
            return self.sharing.select_peer(share_id, peer_id)
                || self.awaiting_peer(share_id, peer_id);
        };
        let Some(transfer) = self.outbound.transfer(share_id, peer_id) else {
            return false;
        };
        if !self.sharing.select_peer(share_id, peer_id)
            && !self.awaiting_peer(share_id, peer_id)
        {
            return false;
        }
        let medium = transfer
            .routes()
            .first()
            .map(|route| medium_name(route.medium()));
        let _observed = self
            .sharing
            .record_observation(share_id, medium, None, None, None);
        if network.send_share(share_id, transfer).is_ok() {
            return true;
        }
        let _failed = self.sharing.fail(share_id);
        false
    }

    pub(super) fn start_pinned_outbound(&mut self, share_id: u64) -> bool {
        let Some(peer_id) = self
            .sharing
            .snapshot()
            .peers()
            .iter()
            .find(|peer| peer.is_pinned())
            .map(|peer| peer.id().to_owned())
            .or_else(|| self.config.pinned_peer_id.clone())
        else {
            return false;
        };
        self.select_peer(share_id, &peer_id)
    }

    fn awaiting_peer(&self, share_id: u64, peer_id: &str) -> bool {
        self.sharing.snapshot().active_share().is_some_and(|share| {
            share.id().get() == share_id
                && share.direction() == Direction::Outbound
                && share.phase() == Phase::AwaitingPeerConsent
                && share.peer().is_some_and(|peer| peer.id() == peer_id)
        })
    }

    fn auto_start_pinned(&mut self, peer_id: &str) {
        if self.config.pinned_peer_id.as_deref() != Some(peer_id) {
            return;
        }
        let Some(share) = self.sharing.snapshot().active_share() else {
            return;
        };
        if share.direction() != Direction::Outbound
            || !matches!(
                share.phase(),
                Phase::WaitingForPeer | Phase::AwaitingPeerConsent
            )
        {
            return;
        }
        let share_id = share.id().get();
        let _started = self.select_peer(share_id, peer_id);
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "Focused unit tests stay beside private event transitions"
)]
mod tests;
