use super::{Error, MAX_PAYLOAD_LENGTH, PayloadKind};
use quickshare_wire::connections::payload_transfer_frame;

fn payload_rejected(reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "payload",
        operation = "validate",
        outcome = "rejected",
        reason,
        "payload rejected"
    );
}

pub(super) fn payload_header_data(
    header: &payload_transfer_frame::PayloadHeader,
) -> Result<(i64, i64, PayloadKind), Error> {
    let id = header.id.ok_or_else(|| {
        payload_rejected("missing_id");
        Error::InvalidPayload
    })?;
    let size = header.total_size.ok_or_else(|| {
        payload_rejected("missing_size");
        Error::InvalidPayload
    })?;
    if !(0..=MAX_PAYLOAD_LENGTH).contains(&size) {
        payload_rejected("size_out_of_bounds");
        return Err(Error::InvalidPayload);
    }
    let kind = match header.r#type {
        Some(value)
            if value
                == payload_transfer_frame::payload_header::PayloadType::Bytes
                    as i32 =>
        {
            PayloadKind::Bytes
        }
        Some(value)
            if value
                == payload_transfer_frame::payload_header::PayloadType::File
                    as i32 =>
        {
            PayloadKind::File
        }
        _ => {
            payload_rejected("unsupported_payload_type");
            return Err(Error::InvalidPayload);
        }
    };
    Ok((id, size, kind))
}
