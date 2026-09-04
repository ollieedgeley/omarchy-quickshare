//! Process entry point for Omarchy Quick Share.

#![expect(
    clippy::multiple_crate_versions,
    reason = "Locked deps pin overlapping getrandom and syn versions"
)]
use clap as _;
#[cfg(test)]
use prost as _;
use std::io;

use quickshare_bluez as _;
use quickshare_connections as _;
use quickshare_control as _;
use quickshare_crypto as _;
use quickshare_network as _;
use quickshare_sharing as _;
use quickshare_storage as _;
use rand_core as _;
use tracing as _;
use tracing_subscriber as _;
use zbus as _;

fn main() -> io::Result<()> {
    omarchy_quickshare::run_from_environment()
}
