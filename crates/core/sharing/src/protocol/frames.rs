use crate::protocol::{PairingStatus, ProtocolError};
use prost::Message as _;
use quickshare_wire::sharing::{
    ConnectionResponseFrame, Frame, PairedKeyEncryptionFrame,
    PairedKeyResultFrame, V1Frame, connection_response_frame,
    paired_key_result_frame, v1_frame,
};
use rand_core::{OsRng, RngCore as _};

pub(in crate::protocol) fn account_free_encryption() -> Frame {
    let mut signed_data = vec![0; 32];
    let mut secret_id_hash = vec![0; 32];
    OsRng.fill_bytes(&mut signed_data);
    OsRng.fill_bytes(&mut secret_id_hash);
    frame(
        v1_frame::FrameType::PairedKeyEncryption,
        V1Frame {
            paired_key_encryption: Some(PairedKeyEncryptionFrame {
                signed_data: Some(signed_data),
                secret_id_hash: Some(secret_id_hash),
                optional_signed_data: None,
                qr_code_handshake_data: None,
            }),
            ..Default::default()
        },
    )
}

pub(in crate::protocol) fn account_free_result() -> Frame {
    frame(
        v1_frame::FrameType::PairedKeyResult,
        V1Frame {
            paired_key_result: Some(PairedKeyResultFrame {
                status: Some(paired_key_result_frame::Status::Unable as i32),
                os_type: None,
            }),
            ..Default::default()
        },
    )
}

pub(in crate::protocol) fn decode_pairing(
    bytes: &[u8],
) -> Result<PairingStatus, ProtocolError> {
    let v1 = Frame::decode(bytes)?
        .v1
        .ok_or(ProtocolError::InvalidFrame)?;
    if let Some(result) = v1.paired_key_result {
        return (result.status
            == Some(paired_key_result_frame::Status::Unable as i32))
        .then_some(PairingStatus::Unable)
        .ok_or(ProtocolError::InvalidFrame);
    }
    v1.paired_key_encryption
        .ok_or(ProtocolError::InvalidFrame)
        .map(|_| PairingStatus::Unable)
}

pub(in crate::protocol) fn is_cancel(
    bytes: &[u8],
) -> Result<bool, ProtocolError> {
    let frame = Frame::decode(bytes)?;
    Ok(frame.version == Some(1)
        && frame.v1.and_then(|v1| v1.r#type)
            == Some(v1_frame::FrameType::Cancel as i32))
}

pub(in crate::protocol) fn decode_response(
    bytes: &[u8],
) -> Result<connection_response_frame::Status, ProtocolError> {
    let response = Frame::decode(bytes)?
        .v1
        .ok_or(ProtocolError::InvalidFrame)?
        .connection_response
        .ok_or(ProtocolError::InvalidFrame)?;
    connection_response_frame::Status::try_from(
        response.status.ok_or(ProtocolError::InvalidFrame)?,
    )
    .map_err(|_| ProtocolError::InvalidFrame)
}

pub(in crate::protocol) fn introduction(name: &str, size: i64) -> Frame {
    use quickshare_wire::sharing::{
        FileMetadata, IntroductionFrame, file_metadata,
    };

    const FILE_ATTACHMENT_ID: i64 = 4;
    const FILE_PAYLOAD_ID: i64 = 3;
    frame(
        v1_frame::FrameType::Introduction,
        V1Frame {
            introduction: Some(IntroductionFrame {
                file_metadata: vec![FileMetadata {
                    id: Some(FILE_ATTACHMENT_ID),
                    mime_type: Some(String::from("application/octet-stream")),
                    name: Some(name.into()),
                    r#type: Some(file_metadata::Type::Document as i32),
                    payload_id: Some(FILE_PAYLOAD_ID),
                    size: Some(size),
                    ..Default::default()
                }],
                start_transfer: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}

pub(in crate::protocol) fn cancel() -> Frame {
    frame(v1_frame::FrameType::Cancel, V1Frame::default())
}

pub(in crate::protocol) fn accept_response() -> Frame {
    frame(
        v1_frame::FrameType::Response,
        V1Frame {
            connection_response: Some(ConnectionResponseFrame {
                status: Some(connection_response_frame::Status::Accept as i32),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}

pub(in crate::protocol) fn reject_response() -> Frame {
    frame(
        v1_frame::FrameType::Response,
        V1Frame {
            connection_response: Some(ConnectionResponseFrame {
                status: Some(connection_response_frame::Status::Reject as i32),
                ..Default::default()
            }),
            ..Default::default()
        },
    )
}

fn frame(kind: v1_frame::FrameType, v1: V1Frame) -> Frame {
    Frame {
        version: Some(1),
        v1: Some(V1Frame {
            r#type: Some(kind as i32),
            ..v1
        }),
    }
}
