//! Shared semantic contract suite.

#![forbid(unsafe_code)]

#[path = "suite/control_contract.rs"]
mod control_contract;
#[path = "suite/scenario_contract.rs"]
mod scenario_contract;
#[path = "suite/sharing_contract.rs"]
mod sharing_contract;
