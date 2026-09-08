//! Versioned local control messages shared by the endpoint and its clients.
//!
//! Each Unix-stream connection carries one request envelope and one response
//! envelope, encoded as newline-delimited JSON. Envelopes pair a command or
//! result with the local compatibility version. Request factories set
//! version 7; the shared client rejects responses with a different version.
//! This version does not describe the peer-to-peer Google wire protocol.
//! Frames contain at most [`MAX_MESSAGE_BYTES`] UTF-8 JSON bytes, excluding
//! their required terminating LF. EOF before LF is a truncated frame.
//! Malformed frames receive `invalid_request`, unsupported versions receive
//! `unsupported_version`, oversized frames receive `message_too_large`, and
//! incomplete requests receive `request_timeout` at the connection deadline.
//! Ordinary connections accept no more than one request, with no work retries.
//! Submissions require an opaque, nonempty ID of at most 128 UTF-8 bytes.
//! Other envelopes omit it. Operation queries carry the original ID inside the
//! command and never resubmit work. Unknown outcomes do not establish that an
//! operation was unaccepted and must not trigger blind retries.
//!
//! An applied response confirms a state change. A snapshot request returns the
//! endpoint's full public state and saved/applied preference status at the time
//! it is handled. Snapshots include outbound discovery, observed peers, the
//! active share, inbound visibility, and durable operations; they are not
//! incremental updates.
//! A ready response only confirms local endpoint readiness.

#![forbid(unsafe_code)]
#![expect(
    clippy::large_enum_variant,
    reason = "Snapshot responses carry full endpoint state locally"
)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream client use"
    )
)]

/// Shared transport for clients of the local endpoint.
pub mod client;
/// Newline-delimited JSON encoding for local control messages.
pub mod codec;
/// Commands accepted by the local endpoint.
pub mod request;
/// Results returned by the local endpoint.
pub mod response;

use core::time::Duration;

/// The local control and CLI compatibility version, not Google's wire version.
pub const PROTOCOL_VERSION: u16 = 7;

/// Maximum UTF-8 JSON frame size, excluding the terminating LF.
pub const MAX_MESSAGE_BYTES: usize = 0x0010_0000;

/// Total time allowed for one connection's request and response I/O.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);
