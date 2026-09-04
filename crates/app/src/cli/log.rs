//! Compact stderr logging for the local daemon.

use std::io;

use super::args::LogLevel;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;

/// Installs compact stderr logging for this process.
pub(super) fn init(log_level: LogLevel) {
    fmt()
        .with_env_filter(env_filter(log_level))
        .with_writer(io::stderr)
        .compact()
        .init();
}

/// Builds the process filter, preferring `RUST_LOG` over the CLI default.
#[must_use]
pub(super) fn env_filter(log_level: LogLevel) -> EnvFilter {
    let rust_log = std::env::var("RUST_LOG").ok();
    env_filter_from(rust_log.as_deref(), log_level)
}

/// Builds a filter from an explicit optional environment value.
#[must_use]
pub(super) fn env_filter_from(
    rust_log: Option<&str>,
    log_level: LogLevel,
) -> EnvFilter {
    rust_log.map_or_else(
        || EnvFilter::new(log_level.as_str()),
        |value| {
            EnvFilter::try_new(value)
                .unwrap_or_else(|_| EnvFilter::new(log_level.as_str()))
        },
    )
}
