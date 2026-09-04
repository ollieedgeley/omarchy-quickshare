//! Conventional Clap command tree for the local endpoint.

use core::iter;
use std::ffi::OsString;

use clap::{Parser, Subcommand, ValueEnum, error::ErrorKind};

const AFTER_HELP: &str = "\
Examples:
  omarchy-quickshare \"hello from Omarchy\"
  omarchy-quickshare ./note.txt
  omarchy-quickshare \"https://example.test/share\"
  omarchy-quickshare status --json
  omarchy-quickshare discover start
  omarchy-quickshare peer pin galaxy-tab
  omarchy-quickshare share select 1 pixel-8
  omarchy-quickshare visibility open
  omarchy-quickshare daemon
  omarchy-quickshare daemon --log-level debug

Logs go to stderr. Follow the user service with:
  journalctl --user -u omarchy-quickshare.service -f
";

/// Omarchy Quick Share command-line interface.
#[derive(Debug, Parser)]
#[command(
    name = "omarchy-quickshare",
    version = env!("CARGO_PKG_VERSION"),
    about = env!("CARGO_PKG_DESCRIPTION"),
    arg_required_else_help = true,
    after_help = AFTER_HELP
)]
pub(super) struct Cli {
    /// Selected subcommand.
    #[command(subcommand)]
    command: Command,
}

impl Cli {
    /// Consumes the parsed tree and returns the selected command.
    pub(super) fn into_command(self) -> Command {
        self.command
    }
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub(super) enum Command {
    /// Queue text, a URL, or a local file or folder for sharing.
    Send {
        /// Send to this observed peer instead of the preferred peer.
        #[arg(long, value_name = "PEER_ID")]
        peer: Option<String>,
        /// Text, URL, filesystem path, or local file URI.
        #[arg(value_name = "CONTENT")]
        content: String,
    },
    /// Print endpoint observations.
    Status {
        /// Write the versioned snapshot envelope as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Confirm that the local endpoint is running.
    Health,
    /// Print the control protocol version.
    ProtocolVersion,
    /// Start or stop outbound peer discovery.
    Discover {
        /// Discovery action.
        #[arg(value_enum, value_name = "ACTION")]
        action: DiscoverAction,
    },
    /// Open or close inbound discoverability.
    Visibility {
        /// Visibility action.
        #[arg(value_enum, value_name = "ACTION")]
        action: VisibilityAction,
    },
    /// Pin or unpin the preferred peer.
    Peer {
        /// Pin action.
        #[command(subcommand)]
        action: PeerCommand,
    },
    /// Act on a queued or inbound share.
    Share {
        /// Share action.
        #[command(subcommand)]
        action: ShareCommand,
    },
    /// Show or update user settings.
    Config {
        /// Config action.
        #[command(subcommand)]
        action: ConfigCommand,
    },
    /// Run the local endpoint.
    Daemon {
        /// Serve deterministic simulated peers.
        #[arg(long)]
        simulate: bool,
        /// Compact stderr log verbosity when `RUST_LOG` is unset.
        #[arg(long, value_enum, default_value_t = LogLevel::Info)]
        log_level: LogLevel,
    },
    /// Inject deterministic simulator events.
    #[command(hide = true)]
    Simulate {
        /// Simulator event.
        #[command(subcommand)]
        action: SimulateCommand,
    },
}

/// Outbound discovery action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(super) enum DiscoverAction {
    /// Start or restart the outbound peer search.
    Start,
    /// Stop the current outbound peer search.
    Stop,
}

/// Inbound visibility action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(super) enum VisibilityAction {
    /// Advertise this endpoint to nearby senders.
    Open,
    /// Stop advertising this endpoint.
    Close,
}

/// Preferred-peer action.
#[derive(Debug, Subcommand)]
pub(super) enum PeerCommand {
    /// Prefer one observed peer for future shares.
    Pin {
        /// Stable peer identifier.
        #[arg(value_name = "PEER_ID")]
        peer_id: String,
    },
    /// Clear the single pinned peer.
    Unpin,
}

/// Share consent and selection actions.
#[derive(Debug, Subcommand)]
pub(super) enum ShareCommand {
    /// Choose a peer for one queued outbound share.
    Select {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
        /// Stable peer identifier.
        #[arg(value_name = "PEER_ID")]
        peer_id: String,
    },
    /// Accept one inbound share.
    Accept {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
    /// Reject one inbound share.
    Reject {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
    /// Cancel one active share.
    Cancel {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
    /// Clear one terminal share from public state.
    Dismiss {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
}

/// User-setting actions.
#[derive(Debug, Subcommand)]
pub(super) enum ConfigCommand {
    /// Print the effective settings as TOML.
    Show,
    /// Update one documented setting.
    Set {
        /// Documented config key.
        #[arg(value_name = "KEY")]
        key: String,
        /// Replacement value.
        #[arg(value_name = "VALUE")]
        value: String,
    },
}

/// Compact stderr log verbosity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub(super) enum LogLevel {
    /// Error events only.
    Error,
    /// Warnings and errors.
    Warn,
    /// Normal operational events.
    #[default]
    Info,
    /// Debugging detail.
    Debug,
    /// Most verbose tracing.
    Trace,
}

impl LogLevel {
    /// Returns the `tracing` directive for this level.
    #[must_use]
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Error => "error",
            Self::Info => "info",
            Self::Trace => "trace",
            Self::Warn => "warn",
        }
    }
}

/// Hidden simulator events.
#[derive(Debug, Subcommand)]
pub(super) enum SimulateCommand {
    /// Expire the current outbound search.
    DiscoveryTimeout,
    /// Fail one active share.
    Fail {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
    /// Offer a deterministic inbound file.
    IncomingFile {
        /// User-visible file name.
        #[arg(value_name = "NAME")]
        name: String,
        /// Declared size in bytes.
        #[arg(value_name = "SIZE_BYTES")]
        size_bytes: u64,
    },
    /// Offer a deterministic inbound text payload.
    IncomingText {
        /// Offered text.
        #[arg(value_name = "TEXT")]
        text: String,
    },
    /// Offer a deterministic inbound URL.
    IncomingUrl {
        /// Offered URL.
        #[arg(value_name = "URL")]
        url: String,
    },
    /// Accept the outbound share as the selected peer.
    PeerAccept {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
    /// Remove one observed peer.
    PeerLost {
        /// Stable peer identifier.
        #[arg(value_name = "PEER_ID")]
        peer_id: String,
    },
    /// Reject the outbound share as the selected peer.
    PeerReject {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
    },
    /// Observe one simulated peer.
    PeerSeen {
        /// Stable peer identifier.
        #[arg(value_name = "PEER_ID")]
        peer_id: String,
        /// User-visible name.
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// Record transfer progress for one share.
    Progress {
        /// Share identifier.
        #[arg(value_name = "SHARE_ID")]
        share_id: u64,
        /// Bytes transferred so far.
        #[arg(value_name = "TRANSFERRED_BYTES")]
        transferred_bytes: u64,
    },
}

/// Parses process arguments after the program name.
pub(super) fn parse<I, T>(arguments: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments = arguments.into_iter().map(Into::into).collect::<Vec<_>>();
    match Cli::try_parse_from(
        iter::once(OsString::from("omarchy-quickshare"))
            .chain(arguments.iter().cloned()),
    ) {
        Err(error)
            if error.kind() == ErrorKind::InvalidSubcommand
                && arguments.len() == 1 =>
        {
            Cli::try_parse_from(
                [OsString::from("omarchy-quickshare"), OsString::from("send")]
                    .into_iter()
                    .chain(arguments),
            )
        }
        result => result,
    }
}
