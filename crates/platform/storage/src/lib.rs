//! Safe local file handling for Quick Share attachments.
#![cfg_attr(
    test,
    expect(
        dead_code_pub_in_binary,
        reason = "Integration tests and app composition use this public API."
    )
)]
#![expect(
    clippy::error_impl_error,
    clippy::pub_use,
    reason = "The stable adapter interface uses crate re-exports and Error"
)]

/// Retained outbound source files.
mod source;
/// Safe staging and atomic publication.
mod staging;

pub use source::OutboundSource;
pub use staging::{ReceiveTarget, StagedFile};

use core::{error, fmt};
use std::io;

/// A local storage operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The completed receive name is already in use.
    DestinationExists,
    /// A peer supplied a non-basename receive name.
    InvalidName,
    /// An outbound attachment is not a regular file with a basename.
    InvalidSource,
    /// The operating system rejected a filesystem operation.
    Io(io::Error),
    /// An outbound attachment changed size after it was accepted.
    SourceChanged,
}

impl fmt::Display for Error {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "Match ergonomics keeps the non-Copy I/O error borrowed"
    )]
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DestinationExists => {
                f.write_str("receive destination already exists")
            }
            Self::InvalidName => f.write_str("receive name is not a basename"),
            Self::InvalidSource => f.write_str("outbound source is not a file"),
            Self::Io(error) => write!(f, "local storage failed: {error}"),
            Self::SourceChanged => f.write_str("outbound source changed size"),
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "The standard Error defaults provide the required behavior"
)]
impl error::Error for Error {}

impl From<io::Error> for Error {
    #[inline]
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
