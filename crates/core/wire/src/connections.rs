//! Nearby Connections protobuf message access.

#![expect(
    clippy::pub_use,
    reason = "This facade is the supported path to Connections messages"
)]
pub use crate::generated::location::nearby::connections::*;
