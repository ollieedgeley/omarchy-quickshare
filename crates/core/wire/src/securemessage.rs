//! Secure Message protobuf message access.

#![expect(
    clippy::pub_use,
    reason = "This facade exposes Secure Message types to protocol code"
)]
pub use crate::generated::securemessage::{
    EcP256PublicKey, EncScheme, GenericPublicKey, Header,
    HeaderAndBodyInternal, PublicKeyType, SecureMessage, SigScheme,
};
