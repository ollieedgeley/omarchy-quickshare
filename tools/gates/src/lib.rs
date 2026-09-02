//! Compile-time contracts used while the application workspace is still empty.

#![forbid(unsafe_code)]

/// Returns whether the repository quality contracts are available.
#[must_use]
#[inline]
pub const fn tooling_is_ready() -> bool {
    true
}

#[cfg(test)]
mod tests;
