//! Process entry point for the standalone Quick Share client.

#![expect(
    clippy::multiple_crate_versions,
    reason = "Locked deps pin overlapping getrandom and syn versions"
)]

use clap as _;
use quickshare_config as _;
use quickshare_control as _;
use quickshare_sharing as _;
use std::io;

fn main() -> io::Result<()> {
    omarchy_quickshare_cli::run_from_environment()
}
