//! Process entry point for the Omarchy Quick Share daemon.

#![expect(
    clippy::pub_with_shorthand,
    clippy::redundant_pub_crate,
    clippy::single_call_fn,
    reason = "Entrypoint keeps rustfmt visibility and focused startup helpers"
)]

mod startup;

#[cfg(test)]
use prost as _;
use std::io;

use quickshare_bluez as _;
use quickshare_config as _;
use quickshare_connections as _;
use quickshare_crypto as _;
use quickshare_network as _;
use quickshare_sharing as _;
use quickshare_storage as _;
use rand_core as _;
use rustix as _;
use serde as _;
use serde_json as _;
use zbus as _;

fn main() -> io::Result<()> {
    startup::run_from_environment()
}
