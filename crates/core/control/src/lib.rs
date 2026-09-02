//! Versioned local control messages shared by the endpoint and its clients.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream client use"
    )
)]

/// Newline-delimited JSON encoding for local control messages.
pub mod codec;
/// Commands accepted by the local endpoint.
pub mod request;
/// Results returned by the local endpoint.
pub mod response;

/// The local control protocol version implemented by this crate.
pub const PROTOCOL_VERSION: u16 = 1;
