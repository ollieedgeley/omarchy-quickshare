//! Secure GCM and device-to-device protobuf message access.

#![expect(
    clippy::pub_use,
    reason = "This facade exposes Secure GCM messages to protocol code"
)]
pub use crate::generated::securegcm::{
    DeviceToDeviceMessage, GcmMetadata, Type, Ukey2ClientFinished,
    Ukey2ClientInit, Ukey2HandshakeCipher, Ukey2Message, Ukey2ServerInit,
};
