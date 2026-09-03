use quickshare_sharing::EndpointSnapshot;
use serde::{Deserialize, Serialize};

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

    /// Creates a successful response for one queued share.
    #[must_use]
    #[inline]
    pub const fn queued() -> Self {
        Self {
            response: Response::Queued,
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
    pub fn snapshot(snapshot: &EndpointSnapshot) -> Self {
        Self {
            response: Response::Snapshot {
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
    /// The endpoint queued the command for processing.
    Queued,
    /// The local endpoint is ready to accept commands.
    Ready,
    /// The endpoint's current public state.
    Snapshot {
        /// State observed through the local control seam.
        snapshot: EndpointSnapshot,
    },
}
