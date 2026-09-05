use super::super::{
    ConnectionOptions, Error, MAX_FRAME_LENGTH, MAX_PAYLOAD_LENGTH,
    PayloadKind, UpgradeCredentials,
};
use bandwidth_upgrade_negotiation_frame::{
    EventType as UpgradeEvent, upgrade_path_info::UpgradePathRequest,
};
use quickshare_wire::connections::{
    BandwidthUpgradeNegotiationFrame, ConnectionRequestFrame,
    ConnectionResponseFrame, DisconnectionFrame, KeepAliveFrame, OfflineFrame,
    PayloadTransferFrame, V1Frame, bandwidth_upgrade_negotiation_frame,
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
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "payload",
            operation = "validate",
            outcome = "rejected",
            reason = "size_out_of_bounds",
            "payload rejected"
        );
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

pub(super) fn upgrade_path_available(
    medium: i32,
    credentials: &UpgradeCredentials,
) -> OfflineFrame {
    let medium_enum = super::super::Medium::from_wire(medium);
    let upgrade_path_info = medium_enum.map_or_else(
        || bandwidth_upgrade_negotiation_frame::UpgradePathInfo {
            medium: Some(medium),
            ..Default::default()
        },
        |value| credentials.into_path_info(value),
    );
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32),
        bandwidth_upgrade_negotiation: Some(BandwidthUpgradeNegotiationFrame {
            event_type: Some(UpgradeEvent::UpgradePathAvailable as i32),
            upgrade_path_info: Some(upgrade_path_info),
            ..Default::default()
        }),
        ..Default::default()
    })
}

pub(super) fn upgrade_failure(medium: i32) -> OfflineFrame {
    upgrade_negotiation(
        bandwidth_upgrade_negotiation_frame::EventType::UpgradeFailure,
        BandwidthUpgradeNegotiationFrame {
            upgrade_path_info: Some(
                bandwidth_upgrade_negotiation_frame::UpgradePathInfo {
                    medium: Some(medium),
                    ..Default::default()
                },
            ),
            ..Default::default()
        },
    )
}

pub(super) fn upgrade_path_request(mediums: &[i32]) -> OfflineFrame {
    upgrade_negotiation(
        bandwidth_upgrade_negotiation_frame::EventType::UpgradePathRequest,
        BandwidthUpgradeNegotiationFrame {
            upgrade_path_info: Some(
                bandwidth_upgrade_negotiation_frame::UpgradePathInfo {
                    upgrade_path_request: Some(UpgradePathRequest {
                        mediums: mediums.to_vec(),
                        medium_meta_data: None,
                    }),
                    ..Default::default()
                },
            ),
            ..Default::default()
        },
    )
}

pub(super) fn last_write_to_prior_channel() -> OfflineFrame {
    upgrade_negotiation(
        bandwidth_upgrade_negotiation_frame::EventType::LastWriteToPriorChannel,
        BandwidthUpgradeNegotiationFrame::default(),
    )
}

pub(super) fn safe_to_close_prior_channel() -> OfflineFrame {
    upgrade_negotiation(
        bandwidth_upgrade_negotiation_frame::EventType::SafeToClosePriorChannel,
        BandwidthUpgradeNegotiationFrame {
            safe_to_close_prior_channel: Some(
                bandwidth_upgrade_negotiation_frame::SafeToClosePriorChannel {
                    sta_frequency: None,
                },
            ),
            ..Default::default()
        },
    )
}

pub(super) fn client_introduction(endpoint_id: &str) -> OfflineFrame {
    upgrade_negotiation(
        bandwidth_upgrade_negotiation_frame::EventType::ClientIntroduction,
        BandwidthUpgradeNegotiationFrame {
            client_introduction: Some(
                bandwidth_upgrade_negotiation_frame::ClientIntroduction {
                    endpoint_id: Some(String::from(endpoint_id)),
                    supports_disabling_encryption: Some(false),
                    last_endpoint_id: None,
                },
            ),
            ..Default::default()
        },
    )
}

pub(super) fn client_introduction_ack() -> OfflineFrame {
    upgrade_negotiation(
        bandwidth_upgrade_negotiation_frame::EventType::ClientIntroductionAck,
        BandwidthUpgradeNegotiationFrame {
            client_introduction_ack: Some(
                bandwidth_upgrade_negotiation_frame::ClientIntroductionAck {},
            ),
            ..Default::default()
        },
    )
}

fn upgrade_negotiation(
    event_type: bandwidth_upgrade_negotiation_frame::EventType,
    frame: BandwidthUpgradeNegotiationFrame,
) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32),
        bandwidth_upgrade_negotiation: Some(BandwidthUpgradeNegotiationFrame {
            event_type: Some(event_type as i32),
            ..frame
        }),
        ..Default::default()
    })
}

fn setup_frame_rejected(reason: &'static str, frame_type: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "setup",
        operation = "receive",
        outcome = "rejected",
        reason,
        frame_type,
        "setup frame rejected"
    );
}

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

pub(super) fn request_data(frame: OfflineFrame) -> Result<(), Error> {
    let Some(v1) = frame.v1 else {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "receive",
            outcome = "rejected",
            reason = "missing_v1",
            frame_type = "connection_request",
            "setup frame rejected"
        );
        return Err(Error::UnexpectedFrame);
    };
    if v1.r#type != Some(v1_frame::FrameType::ConnectionRequest as i32) {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "receive",
            outcome = "rejected",
            reason = "unexpected_frame_type",
            frame_type = "connection_request",
            "setup frame rejected"
        );
        return Err(Error::UnexpectedFrame);
    }
    if v1
        .connection_request
        .is_none_or(|value| value.handshake_data.is_some())
    {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "receive",
            outcome = "rejected",
            reason = "invalid_request",
            frame_type = "connection_request",
            "setup frame rejected"
        );
        return Err(Error::UnexpectedFrame);
    }
    Ok(())
}

pub(super) fn response_data(frame: OfflineFrame) -> Result<(), Error> {
    let Some(v1) = frame.v1 else {
        setup_frame_rejected("missing_v1", "connection_response");
        return Err(Error::UnexpectedFrame);
    };
    if v1.r#type != Some(v1_frame::FrameType::ConnectionResponse as i32) {
        setup_frame_rejected("unexpected_frame_type", "connection_response");
        return Err(Error::UnexpectedFrame);
    }
    let Some(response) = v1.connection_response else {
        setup_frame_rejected("missing_response", "connection_response");
        return Err(Error::UnexpectedFrame);
    };
    let Some(status) = response.response else {
        setup_frame_rejected("missing_status", "connection_response");
        return Err(Error::Rejected);
    };
    if status != connection_response_frame::ResponseStatus::Accept as i32 {
        setup_frame_rejected("peer_rejected", "connection_response");
        return Err(Error::Rejected);
    }
    if response.handshake_data.is_some() {
        setup_frame_rejected(
            "unexpected_handshake_data",
            "connection_response",
        );
        return Err(Error::UnexpectedFrame);
    }
    Ok(())
}

pub(super) fn decoded(
    chunk: payload_transfer_frame::PayloadChunk,
) -> Result<(i64, Vec<u8>, bool), Error> {
    let offset = chunk.offset.unwrap_or(-1);
    let Some(flags) = chunk.flags else {
        payload_rejected("missing_flags");
        return Err(Error::InvalidPayload);
    };
    let last = flags
        & payload_transfer_frame::payload_chunk::Flags::LastChunk as i32
        != 0_i32;
    let bytes = match chunk.body {
        Some(bytes) => bytes,
        None if last => Vec::new(),
        None => {
            payload_rejected("missing_body");
            return Err(Error::InvalidPayload);
        }
    };
    if offset < 0 {
        payload_rejected("invalid_offset");
        return Err(Error::InvalidPayload);
    }
    if bytes.len() > MAX_FRAME_LENGTH {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "payload",
            operation = "validate",
            outcome = "rejected",
            reason = "chunk_too_large",
            byte_count = bytes.len(),
            "payload rejected"
        );
        return Err(Error::InvalidPayload);
    }
    Ok((offset, bytes, last))
}

#[cfg(test)]
mod tests;
