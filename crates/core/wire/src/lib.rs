//! Google Nearby and UKEY2 wire messages for Quick Share.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream client use"
    )
)]

use prost_types as _;

/// Nearby Connections messages.
pub mod connections;
/// Bounded four-byte big-endian frame encoding.
pub mod framing;
#[allow(
    missing_docs,
    unnameable_types,
    unreachable_pub,
    clippy::derive_partial_eq_without_eq,
    clippy::doc_markdown,
    clippy::missing_const_for_fn,
    clippy::must_use_candidate,
    clippy::too_many_lines,
    clippy::too_long_first_doc_paragraph,
    reason = "prost-generated bindings mirror the pinned upstream schema"
)]
mod generated;
/// Secure GCM and device-to-device messages.
pub mod secure_gcm;
/// Secure Message messages.
pub mod secure_message;
/// Secure GCM and device-to-device messages.
pub mod securegcm;
/// Secure Message messages.
pub mod securemessage;
/// Sharing messages used over a Nearby Connection.
pub mod sharing;
/// UKEY2 handshake messages.
pub mod ukey2;
