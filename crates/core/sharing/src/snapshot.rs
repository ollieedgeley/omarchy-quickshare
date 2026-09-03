use crate::attachment::Attachment;

/// A stable identifier assigned by the local endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShareId(u64);

/// The direction of a share relative to the local endpoint.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Direction {
    /// The local endpoint receives content.
    Inbound,
    /// The local endpoint sends content.
    Outbound,
}

/// A user-visible stage in a share's lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Phase {
    /// The endpoint is discovering a peer for the share.
    WaitingForPeer,
}

/// Public state for one active share.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShareSnapshot {
    /// The attachment being exchanged.
    attachment: Attachment,
    /// The direction relative to the local endpoint.
    direction: Direction,
    /// The stable local share identifier.
    id: ShareId,
    /// The current user-visible lifecycle stage.
    phase: Phase,
    /// Total declared attachment bytes.
    total_bytes: u64,
    /// Bytes observed across the transfer seam.
    transferred_bytes: u64,
}

/// Public state for the local endpoint.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EndpointSnapshot {
    /// The share currently displayed by clients.
    active_share: Option<ShareSnapshot>,
}

impl EndpointSnapshot {
    /// Returns the currently active share, if one exists.
    #[must_use]
    #[inline]
    pub const fn active_share(&self) -> Option<&ShareSnapshot> {
        self.active_share.as_ref()
    }

    /// Returns an idle endpoint state.
    #[must_use]
    #[inline]
    #[expect(
        clippy::single_call_fn,
        reason = "Idle state construction belongs to the snapshot owner"
    )]
    pub(crate) const fn idle() -> Self {
        Self { active_share: None }
    }

    /// Creates endpoint state containing one active share.
    #[must_use]
    #[inline]
    #[expect(
        clippy::single_call_fn,
        reason = "Active state construction belongs to the snapshot owner"
    )]
    pub(crate) const fn with_active(active_share: ShareSnapshot) -> Self {
        Self {
            active_share: Some(active_share),
        }
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
    #[expect(
        clippy::single_call_fn,
        reason = "The coordinator exclusively creates active-share snapshots"
    )]
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
            phase,
            total_bytes,
            transferred_bytes: 0,
        }
    }

    /// Returns the current lifecycle phase.
    #[must_use]
    #[inline]
    pub const fn phase(&self) -> Phase {
        self.phase
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
