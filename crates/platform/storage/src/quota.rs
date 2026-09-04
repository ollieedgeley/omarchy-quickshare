use crate::Error;
use rustix::fs::statvfs;
use std::{io, path::Path};

/// Rejects `bytes` when the receive root cannot store that many more bytes.
#[inline]
#[expect(
    clippy::pub_with_shorthand,
    clippy::redundant_pub_crate,
    clippy::single_call_fn,
    reason = "isolates filesystem quota policy from staging mechanics"
)]
pub(crate) fn preflight(directory: &Path, bytes: u64) -> Result<(), Error> {
    if bytes == 0 {
        return Ok(());
    }
    if available_bytes(directory)? < bytes {
        return Err(Error::Quota);
    }
    Ok(())
}

/// Reads available bytes for `directory` from filesystem metadata.
#[inline]
#[expect(
    clippy::single_call_fn,
    reason = "keeps fallible statvfs conversion separate from quota policy"
)]
fn available_bytes(directory: &Path) -> Result<u64, Error> {
    let info = statvfs(directory).map_err(io::Error::from)?;
    Ok(available_from_stat(info.f_bavail, info.f_frsize))
}

/// Converts POSIX `f_bavail` and `f_frsize` into an available byte count.
///
/// Overflow saturates to [`u64::MAX`] because a receive size also fits in
/// `u64`.
#[must_use]
#[inline]
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "keeps overflow behavior directly unit-testable"
    )
)]
const fn available_from_stat(blocks: u64, fragment_size: u64) -> u64 {
    match blocks.checked_mul(fragment_size) {
        Some(bytes) => bytes,
        None => u64::MAX,
    }
}

#[cfg(test)]
mod tests;
