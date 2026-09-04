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

/// Safe inbound basename checks.
mod path;
/// Free-space preflight for inbound shares.
mod quota;
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
    Collision,
    /// An inbound file stopped before its declared size was written.
    Interrupted,
    /// A peer supplied a non-basename receive name.
    InvalidName,
    /// An outbound attachment is not a regular file with a basename.
    InvalidSource,
    /// The operating system rejected a filesystem operation.
    Io(io::Error),
    /// An outbound attachment was replaced or changed after it was accepted.
    Mutation,
    /// The receive root does not have enough free space.
    Quota,
    /// Written bytes did not match the attachment's declared size.
    SizeMismatch,
}

impl fmt::Display for Error {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "Match ergonomics keeps the non-Copy I/O error borrowed"
    )]
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collision => {
                f.write_str("receive destination already exists")
            }
            Self::Interrupted => {
                f.write_str("inbound file was interrupted before completion")
            }
            Self::InvalidName => f.write_str("receive name is not a basename"),
            Self::InvalidSource => f.write_str("outbound source is not a file"),
            Self::Io(error) => write!(f, "local storage failed: {error}"),
            Self::Mutation => {
                f.write_str("outbound source was replaced or changed")
            }
            Self::Quota => {
                f.write_str("not enough free space for the inbound share")
            }
            Self::SizeMismatch => f.write_str(
                "inbound file size does not match the declared size",
            ),
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
