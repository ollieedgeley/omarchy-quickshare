use serde::{Deserialize, Serialize};

/// Content offered in one share.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Attachment {
    /// User-supplied plain text.
    Text {
        /// The exact text supplied by the user.
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
            Self::Text { value } => {
                u64::try_from(value.len()).unwrap_or(u64::MAX)
            }
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
}
