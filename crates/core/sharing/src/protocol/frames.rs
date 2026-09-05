use crate::protocol::{PairingStatus, ProtocolError};
use prost::Message as _;
use quickshare_wire::sharing::{
    ConnectionResponseFrame, Frame, PairedKeyEncryptionFrame,
    PairedKeyResultFrame, V1Frame, connection_response_frame,
    paired_key_result_frame, v1_frame,
};
use rand_core::{OsRng, RngCore as _};

pub(in crate::protocol) fn account_free_encryption() -> Frame {
    let mut signed_data = vec![0; 72];
    let mut secret_id_hash = vec![0; 6];
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
                os_type: Some(0),
            }),
            ..Default::default()
        },
    )
}

pub(in crate::protocol) fn decode_pairing(
    bytes: &[u8],
) -> Result<PairingStatus, ProtocolError> {
    let frame = Frame::decode(bytes).map_err(|error| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "pairing",
            outcome = "rejected",
            reason = "protobuf_decode",
            "protocol_stage"
        );
        ProtocolError::from(error)
    })?;
    let supported_version = frame.version == Some(1);
    let v1 = frame.v1.ok_or_else(|| {
        let reason = if supported_version {
            "missing_v1"
        } else {
            "unsupported_version"
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "pairing",
            outcome = "rejected",
            reason,
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })?;
    if let Some(result) = v1.paired_key_result {
        return (result.status
            == Some(paired_key_result_frame::Status::Unable as i32))
        .then_some(PairingStatus::Unable)
        .ok_or_else(|| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "pairing",
                outcome = "rejected",
                reason = "pairing_status",
                frame_type = "paired_key_result",
                "protocol_stage"
            );
            ProtocolError::InvalidFrame
        });
    }
    v1.paired_key_encryption
        .ok_or_else(|| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "pairing",
                outcome = "rejected",
                reason = "pairing_frame_type",
                "protocol_stage"
            );
            ProtocolError::InvalidFrame
        })
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

pub(in crate::protocol) fn control_event_type(
    bytes: &[u8],
) -> Option<&'static str> {
    let frame = Frame::decode(bytes).ok()?;
    if frame.version != Some(1) {
        return None;
    }
    match v1_frame::FrameType::try_from(frame.v1?.r#type?).ok()? {
        v1_frame::FrameType::UnknownFrameType => None,
        v1_frame::FrameType::Introduction => Some("introduction"),
        v1_frame::FrameType::Response => Some("response"),
        v1_frame::FrameType::PairedKeyEncryption => {
            Some("paired_key_encryption")
        }
        v1_frame::FrameType::PairedKeyResult => Some("paired_key_result"),
        v1_frame::FrameType::CertificateInfo => Some("certificate_info"),
        v1_frame::FrameType::Cancel => Some("cancel"),
        v1_frame::FrameType::ProgressUpdate => Some("progress_update"),
        v1_frame::FrameType::Bindings => Some("bindings"),
    }
}

pub(in crate::protocol) fn decode_response(
    bytes: &[u8],
) -> Result<connection_response_frame::Status, ProtocolError> {
    let frame = Frame::decode(bytes).map_err(|error| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "consent",
            outcome = "rejected",
            reason = "protobuf_decode",
            "protocol_stage"
        );
        ProtocolError::from(error)
    })?;
    let supported_version = frame.version == Some(1);
    let v1 = frame.v1.ok_or_else(|| {
        let reason = if supported_version {
            "missing_v1"
        } else {
            "unsupported_version"
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "consent",
            outcome = "rejected",
            reason,
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })?;
    let frame_type = v1
        .r#type
        .and_then(|value| v1_frame::FrameType::try_from(value).ok());
    let response = v1.connection_response.ok_or_else(|| {
        let reason = match frame_type {
            None | Some(v1_frame::FrameType::UnknownFrameType) => {
                "unknown_frame_type"
            }
            Some(_) => "unexpected_frame_type",
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "consent",
            outcome = "rejected",
            reason,
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })?;
    let status = response.status.ok_or_else(|| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "consent",
            outcome = "rejected",
            reason = "missing_status",
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })?;
    connection_response_frame::Status::try_from(status).map_err(|_| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "consent",
            outcome = "rejected",
            reason = "unknown_status",
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })
}

pub(in crate::protocol) fn consent_result(
    status: connection_response_frame::Status,
) -> Result<(), ProtocolError> {
    let outcome = match status {
        connection_response_frame::Status::Accept => "accepted",
        connection_response_frame::Status::TimedOut => "timed_out",
        connection_response_frame::Status::UnsupportedAttachmentType => {
            "unsupported"
        }
        _ => "rejected",
    };
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "consent",
        operation = "receive",
        outcome,
        event_type = "response",
        "protocol_stage"
    );
    match status {
        connection_response_frame::Status::Accept => Ok(()),
        connection_response_frame::Status::TimedOut => {
            Err(ProtocolError::TimedOut)
        }
        connection_response_frame::Status::UnsupportedAttachmentType => {
            Err(ProtocolError::Unsupported)
        }
        _ => Err(ProtocolError::Rejected),
    }
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

pub(in crate::protocol) fn text_introduction(
    title: &str,
    size: i64,
    kind: quickshare_wire::sharing::text_metadata::Type,
) -> Frame {
    use quickshare_wire::sharing::{IntroductionFrame, TextMetadata};

    const TEXT_ATTACHMENT_ID: i64 = 4;
    const TEXT_PAYLOAD_ID: i64 = 3;
    frame(
        v1_frame::FrameType::Introduction,
        V1Frame {
            introduction: Some(IntroductionFrame {
                text_metadata: vec![TextMetadata {
                    id: Some(TEXT_ATTACHMENT_ID),
                    text_title: Some(title.into()),
                    r#type: Some(kind as i32),
                    payload_id: Some(TEXT_PAYLOAD_ID),
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
    status_response(connection_response_frame::Status::Accept)
}

pub(in crate::protocol) fn reject_response() -> Frame {
    status_response(connection_response_frame::Status::Reject)
}

pub(in crate::protocol) fn timeout_response() -> Frame {
    status_response(connection_response_frame::Status::TimedOut)
}

pub(in crate::protocol) fn unsupported_response() -> Frame {
    status_response(
        connection_response_frame::Status::UnsupportedAttachmentType,
    )
}

fn status_response(status: connection_response_frame::Status) -> Frame {
    frame(
        v1_frame::FrameType::Response,
        V1Frame {
            connection_response: Some(ConnectionResponseFrame {
                status: Some(status as i32),
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

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::inline_modules,
    reason = "Focused wire-shape assertions stay beside private frame builders"
)]
mod tests {
    use super::{account_free_encryption, account_free_result};

    #[test]
    fn account_free_frames_match_google_field_shape() {
        let encryption = account_free_encryption()
            .v1
            .and_then(|frame| frame.paired_key_encryption)
            .expect("paired-key encryption");
        assert_eq!(
            encryption.signed_data.as_deref().map(<[u8]>::len),
            Some(72)
        );
        assert_eq!(
            encryption.secret_id_hash.as_deref().map(<[u8]>::len),
            Some(6)
        );

        let result = account_free_result()
            .v1
            .and_then(|frame| frame.paired_key_result)
            .expect("paired-key result");
        assert_eq!(result.os_type, Some(0));
    }
}
