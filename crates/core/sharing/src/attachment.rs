/// Content offered in one share.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Attachment {
    /// User-supplied plain text.
    Text(String),
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
            Self::Text(value) => u64::try_from(value.len()).unwrap_or(u64::MAX),
        }
    }

    /// Creates a plain-text attachment.
    #[must_use]
    #[inline]
    pub fn text(value: &str) -> Self {
        Self::Text(String::from(value))
    }
}
