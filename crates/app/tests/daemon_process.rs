//! Process-level contracts for the local endpoint daemon.

#![forbid(unsafe_code)]
#![expect(
    clippy::case_sensitive_file_extension_comparisons,
    clippy::let_underscore_untyped,
    clippy::pattern_type_mismatch,
    clippy::redundant_clone,
    clippy::shadow_reuse,
    clippy::shadow_unrelated,
    clippy::single_call_fn,
    reason = "Process contracts keep descriptive command-output bindings"
)]

use clap as _;
use prost as _;
use quickshare_bluez as _;
use quickshare_connections as _;
use quickshare_crypto as _;
use quickshare_storage as _;
use rand_core as _;
use tracing as _;
use tracing_subscriber as _;
use zbus as _;

#[cfg(test)]
#[path = "daemon_process/config_contract.rs"]
mod config_contract;

#[cfg(test)]
#[path = "daemon_process/process_contract.rs"]
mod process_contract;

#[cfg(test)]
#[path = "daemon_process/diagnostic_contract.rs"]
mod diagnostic_contract;
