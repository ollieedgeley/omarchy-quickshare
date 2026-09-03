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

    /// Queues one outbound attachment and returns its stable identifier.
    #[must_use]
    #[inline]
    pub fn queue_outbound(&mut self, attachment: Attachment) -> ShareId {
        let share_id = ShareId::new(self.next_share_id);
        self.next_share_id = self.next_share_id.saturating_add(1);
        let total_bytes = attachment.byte_len();
        self.snapshot = EndpointSnapshot::with_active(ShareSnapshot::new(
            attachment,
            Direction::Outbound,
            share_id,
            Phase::WaitingForPeer,
            total_bytes,
        ));
        share_id
    }

    /// Returns the current public endpoint state.
    #[must_use]
    #[inline]
    pub const fn snapshot(&self) -> &EndpointSnapshot {
        &self.snapshot
    }
}

impl Default for Coordinator {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
