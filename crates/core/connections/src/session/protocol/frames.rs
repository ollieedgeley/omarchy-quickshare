use super::super::{
    ConnectionOptions, Error, MAX_FRAME_LENGTH, MAX_PAYLOAD_LENGTH, PayloadKind,
};
use quickshare_wire::connections::{
    ConnectionRequestFrame, ConnectionResponseFrame, DisconnectionFrame,
    KeepAliveFrame, OfflineFrame, PayloadTransferFrame, V1Frame,
    connection_response_frame, offline_frame, payload_transfer_frame, v1_frame,
};

const fn offline(v1: V1Frame) -> OfflineFrame {
    OfflineFrame {
        version: Some(offline_frame::Version::V1 as i32),
        v1: Some(v1),
    }
}

pub(super) fn request(options: ConnectionOptions) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::ConnectionRequest as i32),
        connection_request: Some(ConnectionRequestFrame {
            endpoint_id: Some(options.id),
            endpoint_info: (!options.info.is_empty()).then_some(options.info),
            endpoint_name: Some(options.name.into_bytes()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

pub(super) fn response() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::ConnectionResponse as i32),
        connection_response: Some(ConnectionResponseFrame {
            response: Some(
                connection_response_frame::ResponseStatus::Accept as i32,
            ),
            ..Default::default()
        }),
        ..Default::default()
    })
}

pub(super) fn payload_header(
    id: i64,
    kind: PayloadKind,
    size: i64,
    name: Option<String>,
) -> Result<payload_transfer_frame::PayloadHeader, Error> {
    if !(0..=MAX_PAYLOAD_LENGTH).contains(&size) {
        return Err(Error::InvalidPayload);
    }
    let ty = match kind {
        PayloadKind::Bytes => {
            payload_transfer_frame::payload_header::PayloadType::Bytes
        }
        PayloadKind::File => {
            payload_transfer_frame::payload_header::PayloadType::File
        }
    };
    Ok(payload_transfer_frame::PayloadHeader {
        id: Some(id),
        r#type: Some(ty as i32),
        total_size: Some(size),
        file_name: name,
        ..Default::default()
    })
}

pub(super) fn data(
    header: payload_transfer_frame::PayloadHeader,
    offset: i64,
    bytes: &[u8],
    last: bool,
) -> OfflineFrame {
    let flags = if last {
        payload_transfer_frame::payload_chunk::Flags::LastChunk as i32
    } else {
        0_i32
    };
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::PayloadTransfer as i32),
        payload_transfer: Some(PayloadTransferFrame {
            packet_type: Some(payload_transfer_frame::PacketType::Data as i32),
            payload_header: Some(header),
            payload_chunk: Some(payload_transfer_frame::PayloadChunk {
                offset: Some(offset),
                body: Some(bytes.to_vec()),
                flags: Some(flags),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

pub(super) fn keepalive(ack: bool, sequence: u32) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::KeepAlive as i32),
        keep_alive: Some(KeepAliveFrame {
            ack: Some(ack),
            seq_num: Some(sequence),
        }),
        ..Default::default()
    })
}

pub(super) fn disconnect() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::Disconnection as i32),
        disconnection: Some(DisconnectionFrame::default()),
        ..Default::default()
    })
}

pub(super) fn request_data(frame: OfflineFrame) -> Result<(), Error> {
    let v1 = frame.v1.ok_or(Error::UnexpectedFrame)?;
    if v1.r#type != Some(v1_frame::FrameType::ConnectionRequest as i32) {
        return Err(Error::UnexpectedFrame);
    }
    v1.connection_request
        .is_some_and(|value| value.handshake_data.is_none())
        .then_some(())
        .ok_or(Error::UnexpectedFrame)
}

pub(super) fn response_data(frame: OfflineFrame) -> Result<(), Error> {
    let v1 = frame.v1.ok_or(Error::UnexpectedFrame)?;
    let response = v1.connection_response.ok_or(Error::UnexpectedFrame)?;
    if v1.r#type != Some(v1_frame::FrameType::ConnectionResponse as i32) {
        return Err(Error::UnexpectedFrame);
    }
    if response.response
        != Some(connection_response_frame::ResponseStatus::Accept as i32)
    {
        return Err(Error::Rejected);
    }
    response
        .handshake_data
        .is_none()
        .then_some(())
        .ok_or(Error::UnexpectedFrame)
}

pub(super) fn decoded(
    chunk: payload_transfer_frame::PayloadChunk,
) -> Result<(i64, Vec<u8>, bool), Error> {
    let offset = chunk.offset.unwrap_or(-1);
    let last = chunk.flags.ok_or(Error::InvalidPayload)?
        & payload_transfer_frame::payload_chunk::Flags::LastChunk as i32
        != 0_i32;
    let bytes = match chunk.body {
        Some(bytes) => bytes,
        None if last => Vec::new(),
        None => return Err(Error::InvalidPayload),
    };
    if offset < 0 || bytes.len() > MAX_FRAME_LENGTH {
        return Err(Error::InvalidPayload);
    }
    Ok((offset, bytes, last))
}

#[cfg(test)]
mod tests;
