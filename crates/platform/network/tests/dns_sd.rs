//! DNS-SD adapter behavior through real multicast sockets.

extern crate alloc;
use tracing as _;

/// DNS-SD integration test cases.
#[cfg(test)]
#[path = "dns_sd/tests.rs"]
mod tests;
