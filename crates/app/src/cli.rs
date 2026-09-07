//! Command-line dispatch for the local endpoint.

mod args;
mod classify;
mod dispatch;
mod log;
mod visibility;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "Focused unit tests stay beside private CLI classification"
)]
mod tests;

pub use dispatch::{run, run_from_environment};

#[cfg(test)]
use classify::{request, request_for_peer};
