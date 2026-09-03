//! Consent and attachment lifecycle for Quick Share.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream coordinator use"
    )
)]
#![expect(
    clippy::pub_use,
    reason = "The architecture requires a stable crate-root interface"
)]

/// Attachment values accepted by a share.
mod attachment;
/// User-visible share lifecycle coordination.
mod coordinator;
/// Public facts about discovered peers.
mod peer;
/// Read-only endpoint and share state.
mod snapshot;

pub use attachment::Attachment;
pub use coordinator::Coordinator;
pub use peer::PeerSnapshot;
pub use snapshot::{
    Direction, DiscoveryState, EndpointSnapshot, Phase, ShareId, ShareSnapshot,
    VisibilityState,
};
