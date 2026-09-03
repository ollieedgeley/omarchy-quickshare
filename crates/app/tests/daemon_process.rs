//! Process-level contracts for the local endpoint daemon.

#![forbid(unsafe_code)]

use quickshare_storage as _;

#[cfg(test)]
#[path = "daemon_process/process_contract.rs"]
mod process_contract;
