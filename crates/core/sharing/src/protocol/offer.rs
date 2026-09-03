use crate::protocol::{IncomingOffer, ProtocolError};
use prost::Message as _;
use quickshare_wire::sharing::Frame;

pub(in crate::protocol) fn decode(
    bytes: &[u8],
) -> Result<IncomingOffer, ProtocolError> {
    let introduction = Frame::decode(bytes)?
        .v1
        .ok_or(ProtocolError::InvalidFrame)?
        .introduction
        .ok_or(ProtocolError::InvalidFrame)?;
    if introduction.file_metadata.len() != 1
        || !introduction.text_metadata.is_empty()
    {
        return Err(ProtocolError::InvalidOffer(
            "expected exactly one file and no text attachments",
        ));
    }
    let file = &introduction.file_metadata[0];
    let (Some(name), Some(size_bytes), Some(payload_id)) =
        (&file.name, file.size, file.payload_id)
    else {
        return Err(ProtocolError::InvalidOffer(
            "missing name, size, or payload identifier",
        ));
    };
    validate(name, size_bytes, payload_id)
}

fn validate(
    name: &str,
    size_bytes: i64,
    payload_id: i64,
) -> Result<IncomingOffer, ProtocolError> {
    if size_bytes < 0 {
        return Err(ProtocolError::InvalidOffer("negative file size"));
    }
    if payload_id == 0 {
        return Err(ProtocolError::InvalidOffer("zero payload identifier"));
    }
    if !safe_name(name) {
        return Err(ProtocolError::InvalidOffer("unsafe file name"));
    }
    Ok(IncomingOffer::new(name.into(), size_bytes, payload_id))
}

pub(in crate::protocol) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
}
