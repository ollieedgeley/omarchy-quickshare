use serde::{Deserialize, Serialize};

use crate::PROTOCOL_VERSION;

/// One versioned command sent to the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The command carried by this envelope.
    request: Request,
    /// The protocol version used to encode the command.
    version: u16,
}

impl Envelope {
    /// Creates a request to submit plain text for sharing.
    #[must_use]
    #[inline]
    pub fn submit_text(text: &str) -> Self {
        Self {
            request: Request::SubmitText {
                text: String::from(text),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit a URL for sharing.
    #[must_use]
    #[inline]
    pub fn submit_url(url: &str) -> Self {
        Self {
            request: Request::SubmitUrl {
                url: String::from(url),
            },
            version: PROTOCOL_VERSION,
        }
    }
}

/// A command supported by the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum Request {
    /// Submit plain text for an outbound share.
    SubmitText {
        /// The exact text supplied by the user.
        text: String,
    },
    /// Submit a URL for an outbound share.
    SubmitUrl {
        /// The exact URL supplied by the user.
        url: String,
    },
}
