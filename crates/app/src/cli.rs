//! Command-line dispatch for the local endpoint.

mod args;
mod classify;
mod dispatch;
mod log;

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "Focused unit tests stay beside private CLI classification"
)]
mod tests;

pub use dispatch::{run, run_from_environment};

#[cfg(test)]
use args::{Command, LogLevel, parse};
#[cfg(test)]
use classify::request;
#[cfg(test)]
use log::env_filter;
