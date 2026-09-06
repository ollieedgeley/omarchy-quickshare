use std::path::PathBuf;

use quickshare_sharing::EndpointSnapshot;
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

    /// Creates a response for a share identifier that is not active.
    #[must_use]
    #[inline]
    pub const fn not_found() -> Self {
        Self {
            response: Response::NotFound,
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
    pub const fn queued(share_id: u64) -> Self {
        Self {
            response: Response::Queued { share_id },
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
    ) -> Self {
        Self {
            response: Response::Snapshot {
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
    /// No active share matched the requested identifier.
    NotFound,
    /// The endpoint's current preference status.
    Preferences {
        /// Saved and activated values observed through the control seam.
        preferences: PreferenceStatus,
    },
    /// The endpoint queued the command for processing.
    Queued {
        /// Stable identifier used by subsequent share actions.
        share_id: u64,
    },
    /// The local endpoint is ready to accept commands.
    Ready,
    /// The endpoint's current public state.
    Snapshot {
        /// Saved and activated preferences accompanying this snapshot.
        preferences: PreferenceStatus,
        /// State observed through the local control seam.
        snapshot: EndpointSnapshot,
    },
}
