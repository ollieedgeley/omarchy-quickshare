//! Shared, test-only support for behavioral and simulator suites.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream suite use"
    )
)]

/// Sharing scenarios and their deterministic fast fake.
pub mod sharing;
