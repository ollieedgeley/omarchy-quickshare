use serde::{Deserialize, Serialize};

/// One versioned response from the local endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The result carried by this envelope.
    response: Response,
    /// The protocol version used to encode the result.
    version: u16,
}

impl Envelope {
    /// Returns the endpoint's response.
    #[must_use]
    #[inline]
    pub const fn response(&self) -> &Response {
        &self.response
    }

    /// Returns the protocol version used by the endpoint.
    #[must_use]
    #[inline]
    pub const fn version(&self) -> u16 {
        self.version
    }
}

/// A result returned by the local endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum Response {
    /// The endpoint queued the command for processing.
    Queued,
}
