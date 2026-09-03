use serde::{Deserialize, Serialize};

/// Content offered in one share.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Attachment {
    /// One regular file offered from local storage.
    File {
        /// User-visible base name sent to the peer.
        name: String,
        /// Declared file content length.
        size_bytes: u64,
    },
    /// User-supplied plain text.
    Text {
        /// The exact text supplied by the user.
        value: String,
    },
    /// User-supplied web address.
    Url {
        /// The exact URL supplied by the user.
        value: String,
    },
}

impl Attachment {
    /// Returns the number of content bytes declared by the attachment.
    #[must_use]
    #[inline]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "Match ergonomics avoids the conflicting ref-pattern lint"
    )]
    pub(crate) fn byte_len(&self) -> u64 {
        match self {
            Self::File { size_bytes, .. } => *size_bytes,
            Self::Text { value } | Self::Url { value } => {
                u64::try_from(value.len()).unwrap_or(u64::MAX)
            }
        }
    }

    /// Creates a file attachment from validated local metadata.
    #[must_use]
    #[inline]
    pub fn file(name: &str, size_bytes: u64) -> Self {
        Self::File {
            name: String::from(name),
            size_bytes,
        }
    }

    /// Creates a plain-text attachment.
    #[must_use]
    #[inline]
    pub fn text(value: &str) -> Self {
        Self::Text {
            value: String::from(value),
        }
    }

    /// Creates a web-address attachment.
    #[must_use]
    #[inline]
    pub fn url(value: &str) -> Self {
        Self::Url {
            value: String::from(value),
        }
    }
}
