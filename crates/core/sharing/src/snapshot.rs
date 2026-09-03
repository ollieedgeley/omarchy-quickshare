use crate::attachment::Attachment;
use crate::peer::PeerSnapshot;
use serde::{Deserialize, Serialize};

/// A stable identifier assigned by the local endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ShareId(u64);

/// The direction of a share relative to the local endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The local endpoint receives content.
    Inbound,
    /// The local endpoint sends content.
    Outbound,
}

/// A user-visible stage in a share's lifecycle.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// The local user must accept or reject an inbound offer.
    AwaitingLocalConsent,
    /// The selected peer must accept or reject an outbound offer.
    AwaitingPeerConsent,
    /// The user stopped the share before completion.
    Cancelled,
    /// Every declared byte crossed the transfer seam.
    Completed,
    /// The transfer stopped because a connection or storage operation failed.
    Failed,
    /// The receiver rejected the offer before transfer.
    Rejected,
    /// Attachment bytes are crossing the transfer seam.
    Transferring,
    /// The endpoint is discovering a peer for the share.
    WaitingForPeer,
}

/// Public state for one active share.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ShareSnapshot {
    /// The attachment being exchanged.
    attachment: Attachment,
    /// The direction relative to the local endpoint.
    direction: Direction,
    /// The stable local share identifier.
    id: ShareId,
    /// Peer selected for this share, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    peer: Option<PeerSnapshot>,
    /// The current user-visible lifecycle stage.
    phase: Phase,
    /// Total declared attachment bytes.
    total_bytes: u64,
    /// Bytes observed across the transfer seam.
    transferred_bytes: u64,
}

/// Public state for the local endpoint.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointSnapshot {
    /// The share currently displayed by clients.
    active_share: Option<ShareSnapshot>,
    /// Peers currently visible to the endpoint.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    peers: Vec<PeerSnapshot>,
}

impl EndpointSnapshot {
    /// Returns the currently active share, if one exists.
    #[must_use]
    #[inline]
    pub const fn active_share(&self) -> Option<&ShareSnapshot> {
        self.active_share.as_ref()
    }

    /// Returns mutable access to the active share inside the coordinator.
    #[inline]
    pub(crate) const fn active_share_mut(
        &mut self,
    ) -> Option<&mut ShareSnapshot> {
        self.active_share.as_mut()
    }

    /// Cancels the active share when its identifier matches.
    pub(crate) const fn cancel(&mut self, share_id: u64) -> bool {
        let Some(active) = self.active_share.as_mut() else {
            return false;
        };
        if active.id.get() != share_id || active.phase.is_terminal() {
            return false;
        }
        active.phase = Phase::Cancelled;
        true
    }

    /// Clears one terminal share after clients have observed its result.
    pub(crate) fn dismiss(&mut self, share_id: u64) -> bool {
        let Some(active) = self.active_share.as_ref() else {
            return false;
        };
        if active.id.get() != share_id || !active.phase.is_terminal() {
            return false;
        }
        self.active_share = None;
        true
    }

    /// Marks one active share as failed.
    pub(crate) const fn fail(&mut self, share_id: u64) -> bool {
        let Some(active) = self.active_share.as_mut() else {
            return false;
        };
        if active.id.get() != share_id || active.phase.is_terminal() {
            return false;
        }
        active.phase = Phase::Failed;
        true
    }

    /// Returns an idle endpoint state.
    #[must_use]
    #[inline]
    #[expect(
        clippy::single_call_fn,
        reason = "Idle state construction belongs to the snapshot owner"
    )]
    pub(crate) const fn idle() -> Self {
        Self {
            active_share: None,
            peers: Vec::new(),
        }
    }

    /// Adds or refreshes one visible peer.
    pub(crate) fn observe_peer(&mut self, peer_id: &str, name: &str) {
        if let Some(peer) =
            self.peers.iter_mut().find(|peer| peer.id() == peer_id)
        {
            peer.rename(name);
            return;
        }
        self.peers.push(PeerSnapshot::new(peer_id, name));
    }

    /// Returns the peer with this stable identifier.
    #[must_use]
    #[inline]
    pub(crate) fn peer(&self, peer_id: &str) -> Option<&PeerSnapshot> {
        self.peers.iter().find(|peer| peer.id() == peer_id)
    }

    /// Returns every peer currently visible to the endpoint.
    #[must_use]
    #[inline]
    pub fn peers(&self) -> &[PeerSnapshot] {
        &self.peers
    }

    /// Pins exactly one known peer.
    pub(crate) fn pin_peer(&mut self, peer_id: &str) -> bool {
        if self.peer(peer_id).is_none() {
            return false;
        }
        for peer in &mut self.peers {
            peer.set_pinned(peer.id() == peer_id);
        }
        true
    }

    /// Returns the preferred outbound peer, when one is visible.
    #[must_use]
    #[inline]
    pub(crate) fn pinned_peer(&self) -> Option<&PeerSnapshot> {
        self.peers.iter().find(|peer| peer.is_pinned())
    }

    /// Removes one peer that is no longer visible during discovery.
    pub(crate) fn remove_peer(&mut self, peer_id: &str) -> bool {
        let Some(index) =
            self.peers.iter().position(|peer| peer.id() == peer_id)
        else {
            return false;
        };
        let _removed = self.peers.remove(index);
        true
    }

    /// Replaces the active share without changing observed peers.
    #[inline]
    pub(crate) fn set_active(&mut self, active_share: ShareSnapshot) {
        self.active_share = Some(active_share);
    }
}

impl Phase {
    /// Returns whether a share has reached a user-visible final result.
    #[inline]
    const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Completed | Self::Failed | Self::Rejected
        )
    }
}

impl ShareId {
    /// Returns the integer representation used by local control clients.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Creates an identifier from the endpoint sequence.
    #[must_use]
    #[inline]
    #[expect(
        clippy::single_call_fn,
        reason = "Share identifiers are constructed only by the coordinator"
    )]
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

impl ShareSnapshot {
    /// Returns the attachment offered by this share.
    #[must_use]
    #[inline]
    pub const fn attachment(&self) -> &Attachment {
        &self.attachment
    }

    /// Returns the direction relative to the local endpoint.
    #[must_use]
    #[inline]
    pub const fn direction(&self) -> Direction {
        self.direction
    }

    /// Returns the stable local share identifier.
    #[must_use]
    #[inline]
    pub const fn id(&self) -> ShareId {
        self.id
    }

    /// Creates a visible active-share snapshot.
    #[must_use]
    #[inline]
    pub(crate) const fn new(
        attachment: Attachment,
        direction: Direction,
        id: ShareId,
        phase: Phase,
        total_bytes: u64,
    ) -> Self {
        Self {
            attachment,
            direction,
            id,
            peer: None,
            phase,
            total_bytes,
            transferred_bytes: 0,
        }
    }

    /// Returns the peer selected for this share.
    #[must_use]
    #[inline]
    pub const fn peer(&self) -> Option<&PeerSnapshot> {
        self.peer.as_ref()
    }

    /// Returns the current lifecycle phase.
    #[must_use]
    #[inline]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Records monotonic transfer progress and completion.
    pub(crate) fn record_progress(&mut self, transferred_bytes: u64) -> bool {
        if self.phase != Phase::Transferring
            || transferred_bytes < self.transferred_bytes
            || transferred_bytes > self.total_bytes
        {
            return false;
        }
        self.transferred_bytes = transferred_bytes;
        if transferred_bytes == self.total_bytes {
            self.phase = Phase::Completed;
        }
        true
    }

    /// Sets the selected peer and lifecycle phase.
    pub(crate) fn select_peer(&mut self, peer: PeerSnapshot, phase: Phase) {
        self.peer = Some(peer);
        self.phase = phase;
    }

    /// Changes the lifecycle phase.
    #[inline]
    pub(crate) const fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }

    /// Returns total declared attachment bytes.
    #[must_use]
    #[inline]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Returns bytes observed across the transfer seam.
    #[must_use]
    #[inline]
    pub const fn transferred_bytes(&self) -> u64 {
        self.transferred_bytes
    }
}
