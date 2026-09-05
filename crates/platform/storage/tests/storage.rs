//! Storage adapter behavior through its public interfaces.

use rustix as _;
use tracing as _;

/// Storage integration test cases.
#[cfg(test)]
#[path = "storage/tests.rs"]
mod tests;
