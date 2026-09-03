use serde::{Deserialize, Serialize};

/// One peer currently visible to the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PeerSnapshot {
    /// Stable identifier used by control clients.
    id: String,
    /// User-visible device name.
    name: String,
    /// Whether new outbound shares prefer this peer.
    pinned: bool,
}

impl PeerSnapshot {
    /// Returns the peer identifier.
    #[must_use]
    #[inline]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns whether this peer is pinned.
    #[must_use]
    #[inline]
    pub const fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Returns the device name shown to the user.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Creates an observed peer.
    #[must_use]
    #[inline]
    #[expect(
        clippy::single_call_fn,
        reason = "Peer construction remains owned by endpoint snapshots"
    )]
    pub(crate) fn new(id: &str, name: &str) -> Self {
        Self {
            id: String::from(id),
            name: String::from(name),
            pinned: false,
        }
    }

    /// Updates the visible name reported by the peer.
    #[inline]
    pub(crate) fn rename(&mut self, name: &str) {
        name.clone_into(&mut self.name);
    }

    /// Changes whether outbound shares prefer this peer.
    #[inline]
    pub(crate) const fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }
}
