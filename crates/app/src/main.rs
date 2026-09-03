//! Process entry point for Omarchy Quick Share.

use std::io;

use quickshare_control as _;
use quickshare_network as _;
use quickshare_sharing as _;
use quickshare_storage as _;

fn main() -> io::Result<()> {
    omarchy_quickshare::run_from_environment()
}
