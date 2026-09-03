use crate::attachment::Attachment;
use crate::snapshot::{
    Direction, EndpointSnapshot, Phase, ShareId, ShareSnapshot,
};

/// Owns the user-visible lifecycle of local shares.
#[derive(Debug)]
pub struct Coordinator {
    /// The identifier reserved for the next submitted share.
    next_share_id: u64,
    /// The current user-visible endpoint state.
    snapshot: EndpointSnapshot,
}

impl Coordinator {
    /// Moves an outbound offer into transfer after peer consent.
    #[inline]
    pub fn accept_by_peer(&mut self, share_id: u64) -> bool {
        self.transition(
            share_id,
            Direction::Outbound,
            Phase::AwaitingPeerConsent,
            Phase::Transferring,
        )
    }

    /// Moves an inbound offer into transfer after local consent.
    #[inline]
    pub fn accept_inbound(&mut self, share_id: u64) -> bool {
        self.transition(
            share_id,
            Direction::Inbound,
            Phase::AwaitingLocalConsent,
            Phase::Transferring,
        )
    }

    /// Cancels the active share when its identifier matches.
    #[inline]
    pub fn cancel(&mut self, share_id: u64) -> bool {
        self.snapshot.cancel(share_id)
    }

    /// Creates an idle endpoint coordinator.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            snapshot: EndpointSnapshot::idle(),
            next_share_id: 1,
        }
    }

    /// Reserves the next stable share identifier.
    #[inline]
    const fn next_id(&mut self) -> ShareId {
        let share_id = ShareId::new(self.next_share_id);
        self.next_share_id = self.next_share_id.saturating_add(1);
        share_id
    }

    /// Adds or refreshes one visible peer.
    #[inline]
    pub fn observe_peer(&mut self, peer_id: &str, name: &str) {
        self.snapshot.observe_peer(peer_id, name);
    }

    /// Creates an inbound offer from a known peer.
    #[must_use]
    #[inline]
    pub fn offer_inbound(
        &mut self,
        attachment: Attachment,
        peer_id: &str,
    ) -> Option<ShareId> {
        let peer = self.snapshot.peer(peer_id)?.clone();
        let share_id = self.next_id();
        let total_bytes = attachment.byte_len();
        let mut share = ShareSnapshot::new(
            attachment,
            Direction::Inbound,
            share_id,
            Phase::AwaitingLocalConsent,
            total_bytes,
        );
        share.select_peer(peer, Phase::AwaitingLocalConsent);
        self.snapshot.set_active(share);
        Some(share_id)
    }

    /// Queues one outbound attachment and returns its stable identifier.
    #[must_use]
    #[inline]
    pub fn queue_outbound(&mut self, attachment: Attachment) -> ShareId {
        let share_id = self.next_id();
        let total_bytes = attachment.byte_len();
        self.snapshot.set_active(ShareSnapshot::new(
            attachment,
            Direction::Outbound,
            share_id,
            Phase::WaitingForPeer,
            total_bytes,
        ));
        share_id
    }

    /// Records monotonic payload progress for the active transfer.
    #[inline]
    pub fn record_progress(
        &mut self,
        share_id: u64,
        transferred_bytes: u64,
    ) -> bool {
        let Some(active) = self.snapshot.active_share_mut() else {
            return false;
        };
        active.id().get() == share_id
            && active.record_progress(transferred_bytes)
    }

    /// Rejects an outbound offer on behalf of the peer.
    #[inline]
    pub fn reject_by_peer(&mut self, share_id: u64) -> bool {
        self.transition(
            share_id,
            Direction::Outbound,
            Phase::AwaitingPeerConsent,
            Phase::Rejected,
        )
    }

    /// Rejects an inbound offer on behalf of the local user.
    #[inline]
    pub fn reject_inbound(&mut self, share_id: u64) -> bool {
        self.transition(
            share_id,
            Direction::Inbound,
            Phase::AwaitingLocalConsent,
            Phase::Rejected,
        )
    }

    /// Selects one observed peer for the active outbound share.
    #[inline]
    pub fn select_peer(&mut self, share_id: u64, peer_id: &str) -> bool {
        let Some(peer) = self.snapshot.peer(peer_id).cloned() else {
            return false;
        };
        let Some(active) = self.snapshot.active_share_mut() else {
            return false;
        };
        if active.id().get() != share_id
            || active.direction() != Direction::Outbound
            || active.phase() != Phase::WaitingForPeer
        {
            return false;
        }
        active.select_peer(peer, Phase::AwaitingPeerConsent);
        true
    }

    /// Returns the current public endpoint state.
    #[must_use]
    #[inline]
    pub const fn snapshot(&self) -> &EndpointSnapshot {
        &self.snapshot
    }

    /// Applies a lifecycle transition when all state predicates match.
    fn transition(
        &mut self,
        share_id: u64,
        direction: Direction,
        from: Phase,
        to: Phase,
    ) -> bool {
        let Some(active) = self.snapshot.active_share_mut() else {
            return false;
        };
        if active.id().get() != share_id
            || active.direction() != direction
            || active.phase() != from
        {
            return false;
        }
        active.set_phase(to);
        true
    }
}

impl Default for Coordinator {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
