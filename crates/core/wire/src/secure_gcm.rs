//! Secure GCM and device-to-device protobuf message access.

#![expect(
    clippy::pub_use,
    reason = "This facade is the supported path to Secure GCM messages"
)]
pub use crate::generated::securegcm::*;
