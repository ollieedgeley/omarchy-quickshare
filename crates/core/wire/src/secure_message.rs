//! Secure Message protobuf message access.

#![expect(
    clippy::pub_use,
    reason = "This facade is the supported path to Secure Message types"
)]
pub use crate::generated::securemessage::*;
