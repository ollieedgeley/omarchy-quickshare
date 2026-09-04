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
    let introduction = Frame::decode(bytes)?
        .v1
        .ok_or(ProtocolError::InvalidFrame)?
        .introduction
        .ok_or(ProtocolError::InvalidFrame)?;
    if !introduction.wifi_credentials_metadata.is_empty()
        || !introduction.stream_metadata.is_empty()
    {
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
        _ => Err(ProtocolError::InvalidOffer(
            "expected exactly one file, text, or app attachment",
        )),
    }
}

fn decode_file(file: &FileMetadata) -> Result<IncomingOffer, ProtocolError> {
    let (Some(name), Some(size_bytes), Some(payload_id)) =
        (&file.name, file.size, file.payload_id)
    else {
        return Err(ProtocolError::InvalidOffer(
            "missing name, size, or payload identifier",
        ));
    };
    if size_bytes < 0 {
        return Err(ProtocolError::InvalidOffer("negative file size"));
    }
    validate_payload_id(payload_id)?;
    if !safe_name(name) {
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
        return Err(ProtocolError::InvalidOffer(
            "misaligned app file payloads",
        ));
    }
    let sizes = match app.file_size.as_slice() {
        [] if count == 1 => {
            vec![app.size.ok_or(ProtocolError::InvalidOffer(
                "missing name, size, or payload identifier",
            ))?]
        }
        sizes if sizes.len() == count => sizes.to_vec(),
        _ => {
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
            return Err(ProtocolError::InvalidOffer(
                "duplicate app payload identifier",
            ));
        }
        let size_bytes = sizes[index];
        if size_bytes < 0 {
            return Err(ProtocolError::InvalidOffer("negative file size"));
        }
        validate_payload_id(payload_id)?;
        let name = &app.file_name[index];
        if !safe_name(name) {
            return Err(ProtocolError::InvalidOffer("unsafe file name"));
        }
        total = total
            .checked_add(size_bytes)
            .ok_or(ProtocolError::InvalidOffer("invalid app size"))?;
        files.push(IncomingFile::new(name.clone(), size_bytes, payload_id));
    }
    if let Some(declared) = app.size
        && declared != total
    {
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
        return Err(ProtocolError::InvalidOffer(
            "missing title, size, type, or payload identifier",
        ));
    };
    if title.is_empty() {
        return Err(ProtocolError::InvalidOffer("empty text title"));
    }
    if !(0..=MAX_TEXT_BYTES).contains(&size_bytes) {
        return Err(ProtocolError::InvalidOffer("invalid text size"));
    }
    validate_payload_id(payload_id)?;
    let kind = match text_metadata::Type::try_from(kind) {
        Ok(text_metadata::Type::Text) => OfferKind::Text,
        Ok(text_metadata::Type::Url) => OfferKind::Url,
        _ => {
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
