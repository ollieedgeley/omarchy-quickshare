//! Account-free encrypted-stream setup shared by raw-peer tests.

use super::pairing::{INITIATOR_RANDOM, INITIATOR_SECRET};
use quickshare_connections::{Connection, ConnectionIo, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_sharing::SharingSession;
use quickshare_wire::sharing::{
    Frame, PairedKeyEncryptionFrame, V1Frame, v1_frame,
};

pub fn account_free_encryption() -> Frame {
    Frame {
        version: Some(1_i32),
        v1: Some(V1Frame {
            r#type: Some(i32::from(v1_frame::FrameType::PairedKeyEncryption)),
            paired_key_encryption: Some(PairedKeyEncryptionFrame {
                signed_data: Some(vec![0; 72]),
                secret_id_hash: Some(vec![0; 6]),
                optional_signed_data: None,
                qr_code_handshake_data: None,
            }),
            ..Default::default()
        }),
    }
}

pub fn connect_paired_initiator<Stream>(stream: Stream) -> SharingSession
where
    Stream: ConnectionIo + 'static,
{
    let connection = Connection::connect_io(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    session
}
