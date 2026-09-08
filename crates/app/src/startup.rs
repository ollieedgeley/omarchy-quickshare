//! Argument parsing and process setup for the standalone daemon.

mod log;

use std::env;
use std::io::{self, Write as _};
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use omarchy_quickshare::daemon;
use quickshare_control::PROTOCOL_VERSION;

/// Options accepted by the daemon without involving the control CLI.
#[derive(Debug, Parser)]
#[command(name = "omarchy-quickshare-daemon", version, about)]
struct Options {
    /// Compact stderr log verbosity, overridden by `RUST_LOG`.
    #[arg(long, value_enum, default_value_t = LogLevel::Info)]
    log_level: LogLevel,
    /// Prints the local control protocol version and exits.
    #[arg(long)]
    protocol_version: bool,
    /// Runs a deterministic local peer instead of the production network.
    #[arg(long, hide = true)]
    simulate: bool,
}

/// Compact stderr log verbosity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum LogLevel {
    /// Debugging detail.
    Debug,
    /// Error events only.
    Error,
    /// Normal operational events.
    #[default]
    Info,
    /// Most verbose tracing.
    Trace,
    /// Warnings and errors.
    Warn,
}

impl LogLevel {
    /// Returns the `tracing` directive for this level.
    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Error => "error",
            Self::Info => "info",
            Self::Trace => "trace",
            Self::Warn => "warn",
        }
    }
}

/// Starts the daemon after handling arguments that need no runtime directory.
pub(super) fn run_from_environment() -> io::Result<()> {
    let options = Options::parse();
    if options.protocol_version {
        return writeln!(io::stdout().lock(), "{PROTOCOL_VERSION}");
    }
    let socket_path = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|root| root.join("omarchy-quickshare/control.sock"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not available",
            )
        })?;
    log::init(options.log_level);
    tracing::info!(simulate = options.simulate, "starting daemon");
    if options.simulate {
        daemon::run_simulated(&socket_path)
    } else {
        daemon::run(&socket_path)
    }
}
