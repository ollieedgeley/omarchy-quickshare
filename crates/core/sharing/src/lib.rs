//! Consent and attachment lifecycle for Quick Share.

#![forbid(unsafe_code)]
#![expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Sharing frames follow protocol chronology"
)]
#![expect(
    clippy::as_conversions,
    reason = "prost protocol enum fields use i32 wire values"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "Validated advertisement offsets advance through bounded wire data"
)]
#![expect(
    clippy::default_numeric_fallback,
    reason = "Advertisement bit positions are defined by the wire format"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "Public protocol result types reserve forward-compatible variants"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "Advertisement decoding validates its wire layout before access"
)]
#![expect(
    clippy::let_underscore_untyped,
    reason = "The account-free pairing result is validated then discarded"
)]
#![expect(
    clippy::min_ident_chars,
    reason = "mDNS instance bytes retain their compact fixed-layout labels"
)]
#![expect(
    clippy::missing_asserts_for_indexing,
    reason = "Invalid advertisement lengths return errors instead of panicking"
)]
#![expect(
    clippy::missing_trait_methods,
    reason = "Error uses the standard default trait methods"
)]
#![expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed protocol-state matches retain readable enum patterns"
)]
#![expect(
    clippy::renamed_function_params,
    reason = "Debug formatter names remain descriptive"
)]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "Unknown Connections events are invalid Sharing frames"
)]
#![expect(
    clippy::map_err_ignore,
    reason = "Malformed peer frames map to privacy-safe protocol failures"
)]
#![cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "Private sharing stages follow public operations"
    )
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "Sharing protocol methods avoid forced cross-crate inlining"
)]
#![expect(
    clippy::single_call_fn,
    reason = "Named sharing frame stages remain protocol-auditable"
)]
#![expect(
    clippy::std_instead_of_alloc,
    reason = "Sharing uses std networking and randomness facilities"
)]
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

#[cfg(test)]
use quickshare_crypto as _;

/// Attachment values accepted by a share.
mod attachment;
/// User-visible share lifecycle coordination.
mod coordinator;
/// Public facts about discovered peers.
mod peer;
/// LAN advertisements and Sharing sessions.
mod protocol;
/// Read-only endpoint and share state.
mod snapshot;

pub use attachment::Attachment;
pub use coordinator::Coordinator;
pub use peer::PeerSnapshot;
pub use protocol::{
    EndpointInfo, IncomingOffer, MdnsInstance, OfferKind, PairingStatus,
    ProtocolError, SharingSession,
};
pub use snapshot::{
    Direction, DiscoveryState, EndpointSnapshot, Phase, ShareId, ShareSnapshot,
    VisibilityState,
};
