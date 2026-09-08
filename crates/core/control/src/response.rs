use std::path::PathBuf;

use quickshare_sharing::{Direction, EndpointSnapshot, Phase};
use serde::{Deserialize, Serialize};

/// Preference values represented by the local control protocol.
#[expect(
    clippy::exhaustive_structs,
    reason = "Preference values are a closed versioned local wire encoding"
)]
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceValues {
    /// Local consent deadline in seconds.
    pub consent_timeout_secs: u64,
    /// Configured device name, or the host-derived name when absent.
    pub device_name: Option<String>,
    /// Whether inbound discoverability is enabled.
    pub discoverable: bool,
    /// Outbound discovery deadline in seconds.
    pub discovery_timeout_secs: u64,
    /// Preferred outbound peer, when configured.
    pub pinned_peer_id: Option<String>,
    /// Whether selecting a peer may read the clipboard.
    pub read_clipboard_on_select: bool,
    /// Directory receiving completed inbound files.
    pub receive_directory: PathBuf,
    /// Transfer deadline in seconds.
    pub transfer_timeout_secs: u64,
}

/// Saved preferences and the last successfully activated values.
#[expect(
    clippy::exhaustive_structs,
    reason = "Preference status is a closed versioned local wire encoding"
)]
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreferenceStatus {
    /// Last activated values, or absent before the first activation.
    pub applied: Option<PreferenceValues>,
    /// Current persistence or activation failure.
    pub error: Option<String>,
    /// Whether saved values are awaiting activation.
    pub pending: bool,
    /// Current document values, or absent when the document is invalid.
    pub saved: Option<PreferenceValues>,
}

/// Durable public outcome of one accepted operation.
#[expect(
    clippy::exhaustive_structs,
    reason = "Operation records are a closed versioned local wire encoding"
)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRecord {
    /// Direction relative to this endpoint.
    pub direction: Direction,
    /// Whether the user dismissed this result.
    pub dismissed: bool,
    /// Lossless decimal local share identifier.
    pub id_string: String,
    /// Last durably recorded lifecycle phase.
    pub phase: Phase,
    /// Safe next action after a terminal outcome.
    pub recovery_guidance: Option<String>,
    /// Original caller identity, absent for inbound shares.
    pub request_id: Option<String>,
    /// Stable terminal outcome reason.
    pub terminal_reason: Option<String>,
    /// Declared payload size.
    pub total_bytes: u64,
    /// Payload bytes observed at the transfer seam.
    pub transferred_bytes: u64,
}

/// One versioned response from the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The result carried by this envelope.
    response: Response,
    /// The protocol version used to encode the result.
    version: u16,
}

impl Envelope {
    /// Creates a response confirming a state-changing action.
    #[must_use]
    #[inline]
    pub const fn applied() -> Self {
        Self {
            response: Response::Applied,
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a response confirming a cancelled share.
    #[must_use]
    #[inline]
    pub const fn cancelled() -> Self {
        Self {
            response: Response::Cancelled,
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a structured rejection of a control message.
    #[must_use]
    #[inline]
    pub const fn error(code: ErrorCode) -> Self {
        Self {
            response: Response::Error { code },
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a response for a share identifier that is not active.
    #[must_use]
    #[inline]
    pub const fn not_found() -> Self {
        Self {
            response: Response::NotFound,
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a reconciliation response for an accepted operation.
    #[must_use]
    #[inline]
    pub const fn operation(operation: OperationRecord) -> Self {
        Self {
            response: Response::Operation { operation },
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a response containing the current preference status.
    #[must_use]
    #[inline]
    pub const fn preferences(preferences: PreferenceStatus) -> Self {
        Self {
            response: Response::Preferences { preferences },
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a successful response for one queued share.
    #[must_use]
    #[inline]
    pub const fn queued(operation: OperationRecord) -> Self {
        Self {
            response: Response::Queued { operation },
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Creates a response for a ready local endpoint.
    #[must_use]
    #[inline]
    pub const fn ready() -> Self {
        Self {
            response: Response::Ready,
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Returns the endpoint's response.
    #[must_use]
    #[inline]
    pub const fn response(&self) -> &Response {
        &self.response
    }

    /// Creates a response containing the endpoint's public state.
    #[must_use]
    #[inline]
    pub fn snapshot(
        snapshot: &EndpointSnapshot,
        preferences: &PreferenceStatus,
        operations: &[OperationRecord],
    ) -> Self {
        Self {
            response: Response::Snapshot {
                operations: operations.to_vec(),
                preferences: preferences.clone(),
                snapshot: snapshot.clone(),
            },
            version: crate::PROTOCOL_VERSION,
        }
    }

    /// Returns the protocol version used by the endpoint.
    #[must_use]
    #[inline]
    pub const fn version(&self) -> u16 {
        self.version
    }
}

/// A result returned by the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum Response {
    /// The endpoint applied the requested state change.
    Applied,
    /// The endpoint cancelled the requested share.
    Cancelled,
    /// The endpoint could not complete the control exchange.
    Error {
        /// Stable machine-readable rejection reason.
        code: ErrorCode,
    },
    /// No active share matched the requested identifier.
    NotFound,
    /// Last durable state of an accepted operation.
    Operation {
        /// Reconciled operation, without resubmission.
        operation: OperationRecord,
    },
    /// The endpoint's current preference status.
    Preferences {
        /// Saved and activated values observed through the control seam.
        preferences: PreferenceStatus,
    },
    /// The endpoint queued the command for processing.
    Queued {
        /// Durably accepted operation.
        operation: OperationRecord,
    },
    /// The local endpoint is ready to accept commands.
    Ready,
    /// The endpoint's current public state.
    Snapshot {
        /// Retained durable operation outcomes.
        operations: Vec<OperationRecord>,
        /// Saved and activated preferences accompanying this snapshot.
        preferences: PreferenceStatus,
        /// State observed through the local control seam.
        snapshot: EndpointSnapshot,
    },
}

/// Stable failures at the local control transport boundary.
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Keep numeric discriminants stable for existing Rust clients"
)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ErrorCode {
    /// The caller identity already belongs to different submitted content.
    Conflict,
    /// The endpoint cannot accept another operation now.
    Busy,
    /// Durable storage failed before acceptance could be confirmed.
    PersistenceFailed,
    /// Acceptance or outcome cannot be established; never implies safe retry.
    OutcomeUnknown,
    /// The frame is not a valid request envelope.
    InvalidRequest,
    /// The envelope uses another protocol version.
    UnsupportedVersion,
    /// The request or response JSON frame exceeds the byte limit.
    MessageTooLarge,
    /// The connection did not complete within its deadline.
    RequestTimeout,
}
