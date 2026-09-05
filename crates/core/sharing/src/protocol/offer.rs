use super::types::IncomingFile;
use crate::protocol::{IncomingOffer, OfferKind, ProtocolError};
use prost::Message as _;
use quickshare_wire::sharing::{
    AppMetadata, FileMetadata, Frame, TextMetadata, file_metadata,
    text_metadata,
};

const MAX_TEXT_BYTES: i64 = 1024 * 1024;

pub(in crate::protocol) fn decode(
    bytes: &[u8],
) -> Result<IncomingOffer, ProtocolError> {
    let frame = Frame::decode(bytes).map_err(|error| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
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
            operation = "offer",
            outcome = "rejected",
            reason,
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })?;
    let frame_type = v1.r#type.and_then(|value| {
        quickshare_wire::sharing::v1_frame::FrameType::try_from(value).ok()
    });
    let introduction = v1.introduction.ok_or_else(|| {
        let reason = match frame_type {
            None
            | Some(
                quickshare_wire::sharing::v1_frame::FrameType::UnknownFrameType,
            ) => "unknown_frame_type",
            Some(_) => "unexpected_frame_type",
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason,
            "protocol_stage"
        );
        ProtocolError::InvalidFrame
    })?;
    if !introduction.wifi_credentials_metadata.is_empty()
        || !introduction.stream_metadata.is_empty()
    {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "unsupported_attachment_kind",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer("unsupported attachment kind"));
    }
    match (
        introduction.file_metadata.len(),
        introduction.text_metadata.len(),
        introduction.app_metadata.len(),
    ) {
        (1, 0, 0) => decode_file(&introduction.file_metadata[0]),
        (0, 1, 0) => decode_text(&introduction.text_metadata[0]),
        (0, 0, 1) => decode_app(&introduction.app_metadata[0]),
        _ => {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "attachment_count",
                "protocol_stage"
            );
            Err(ProtocolError::InvalidOffer(
                "expected exactly one file, text, or app attachment",
            ))
        }
    }
}

fn decode_file(file: &FileMetadata) -> Result<IncomingOffer, ProtocolError> {
    let (Some(name), Some(size_bytes), Some(payload_id)) =
        (&file.name, file.size, file.payload_id)
    else {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "missing_file_fields",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer(
            "missing name, size, or payload identifier",
        ));
    };
    if size_bytes < 0 {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "negative_size",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer("negative file size"));
    }
    validate_payload_id(payload_id)?;
    if !safe_name(name) {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "unsafe_name",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer("unsafe file name"));
    }
    let kind = match file.r#type {
        Some(value) if value == file_metadata::Type::AndroidApp as i32 => {
            OfferKind::AndroidApp
        }
        _ => OfferKind::File,
    };
    Ok(IncomingOffer::new(
        kind,
        name.clone(),
        size_bytes,
        payload_id,
    ))
}

fn decode_app(app: &AppMetadata) -> Result<IncomingOffer, ProtocolError> {
    let count = app.payload_id.len();
    if count == 0 || app.file_name.len() != count {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "app_payload_alignment",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer(
            "misaligned app file payloads",
        ));
    }
    let sizes = match app.file_size.as_slice() {
        [] if count == 1 => vec![app.size.ok_or_else(|| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "missing_app_size",
                "protocol_stage"
            );
            ProtocolError::InvalidOffer(
                "missing name, size, or payload identifier",
            )
        })?],
        sizes if sizes.len() == count => sizes.to_vec(),
        _ => {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "app_payload_alignment",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidOffer(
                "misaligned app file payloads",
            ));
        }
    };
    let mut files = Vec::with_capacity(count);
    let mut total = 0_i64;
    for index in 0..count {
        let payload_id = app.payload_id[index];
        if app.payload_id[..index].contains(&payload_id) {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "duplicate_payload_id",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidOffer(
                "duplicate app payload identifier",
            ));
        }
        let size_bytes = sizes[index];
        if size_bytes < 0 {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "negative_size",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidOffer("negative file size"));
        }
        validate_payload_id(payload_id)?;
        let name = &app.file_name[index];
        if !safe_name(name) {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "unsafe_name",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidOffer("unsafe file name"));
        }
        total = total.checked_add(size_bytes).ok_or_else(|| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "total_size_overflow",
                "protocol_stage"
            );
            ProtocolError::InvalidOffer("invalid app size")
        })?;
        files.push(IncomingFile::new(name.clone(), size_bytes, payload_id));
    }
    if let Some(declared) = app.size
        && declared != total
    {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "app_size_mismatch",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer(
            "app size does not match payloads",
        ));
    }
    let offer = IncomingOffer::with_files(OfferKind::AndroidApp, files, total);
    Ok(match app.package_name.as_deref() {
        Some(package_name) if !package_name.is_empty() => {
            offer.with_package_name(package_name.into())
        }
        _ => offer,
    })
}

fn decode_text(text: &TextMetadata) -> Result<IncomingOffer, ProtocolError> {
    let (Some(title), Some(size_bytes), Some(payload_id), Some(kind)) =
        (&text.text_title, text.size, text.payload_id, text.r#type)
    else {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "missing_text_fields",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer(
            "missing title, size, type, or payload identifier",
        ));
    };
    if title.is_empty() {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "empty_text",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer("empty text title"));
    }
    if !(0..=MAX_TEXT_BYTES).contains(&size_bytes) {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "invalid_text_size",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer("invalid text size"));
    }
    validate_payload_id(payload_id)?;
    let kind = match text_metadata::Type::try_from(kind) {
        Ok(text_metadata::Type::Text) => OfferKind::Text,
        Ok(text_metadata::Type::Url) => OfferKind::Url,
        _ => {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "unsupported_text_type",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidOffer(
                "unsupported text attachment type",
            ));
        }
    };
    Ok(IncomingOffer::new(
        kind,
        title.into(),
        size_bytes,
        payload_id,
    ))
}

fn validate_payload_id(payload_id: i64) -> Result<(), ProtocolError> {
    if payload_id == 0 {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "validation",
            operation = "offer",
            outcome = "rejected",
            reason = "zero_payload_id",
            "protocol_stage"
        );
        return Err(ProtocolError::InvalidOffer("zero payload identifier"));
    }
    Ok(())
}

pub(in crate::protocol) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
}

pub(in crate::protocol) const fn max_text_bytes() -> i64 {
    MAX_TEXT_BYTES
}
