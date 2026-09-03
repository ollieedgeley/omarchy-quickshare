//! UKEY2 protobuf message access.

#![expect(
    clippy::module_name_repetitions,
    clippy::pub_use,
    reason = "This facade preserves UKEY2 schema identifiers"
)]
pub use crate::generated::securegcm::{
    Ukey2Alert, Ukey2ClientFinished, Ukey2ClientInit, Ukey2HandshakeCipher,
    Ukey2Message, Ukey2ServerInit, ukey2_client_init, ukey2_message,
};
