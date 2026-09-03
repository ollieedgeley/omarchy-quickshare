//! Sharing protobuf message access.

#![expect(
    clippy::pub_use,
    reason = "This facade is the supported path to Sharing messages"
)]

/// Nearby Sharing telemetry enums imported by the wire format.
pub use crate::generated::location::nearby::proto::sharing as telemetry;
/// Nearby Sharing settings enums.
pub use crate::generated::nearby::sharing::proto as enums;
pub use crate::generated::nearby::sharing::service::proto::*;
