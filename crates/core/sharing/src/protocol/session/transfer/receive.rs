use super::SharingSession;
use crate::protocol::{IncomingOffer, ProtocolError, frames};
use quickshare_connections::Event;
use std::io::Write;

impl SharingSession {
    pub(super) fn receive_file_header(
        &mut self,
        offer: &IncomingOffer,
    ) -> Result<(i64, u64), ProtocolError> {
        let mut skipped_control_reported = false;
        loop {
            match self.next_transfer_event_for(offer.payload_id())? {
                Event::Bytes { bytes, .. } => {
                    if let Some(event_type) = frames::control_event_type(&bytes)
                    {
                        if event_type == "cancel" {
                            tracing::debug!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "cancelled",
                                reason = "remote",
                                event_type,
                                "protocol_stage"
                            );
                            return Err(ProtocolError::Cancelled);
                        }
                        if skipped_control_reported {
                            tracing::trace!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "skipped",
                                event_type,
                                "protocol_stage"
                            );
                        } else {
                            tracing::debug!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "skipped",
                                event_type,
                                "protocol_stage"
                            );
                            skipped_control_reported = true;
                        }
                    } else {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "control",
                            operation = "demux",
                            outcome = "rejected",
                            reason = "malformed",
                            event_type = "unknown",
                            "protocol_stage"
                        );
                        return Err(ProtocolError::InvalidPayload);
                    }
                }
                Event::FileHeader {
                    id,
                    total_size,
                    name,
                } => {
                    let id_matches_expected = id == offer.payload_id();
                    let size_matches_expected =
                        total_size == offer.size_bytes();
                    let name_matches_expected =
                        name.as_deref() == Some(offer.name());
                    if !id_matches_expected
                        || !size_matches_expected
                        || !name_matches_expected
                    {
                        let reason = if !id_matches_expected {
                            "id_mismatch"
                        } else if !size_matches_expected {
                            "size_mismatch"
                        } else {
                            "name_mismatch"
                        };
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "validation",
                            operation = "payload",
                            outcome = "rejected",
                            reason,
                            id_matches_expected,
                            size_matches_expected,
                            byte_count = total_size,
                            "protocol_stage"
                        );
                        return Err(ProtocolError::InvalidPayload);
                    }
                    let size = u64::try_from(total_size).map_err(|_| {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "validation",
                            operation = "payload",
                            outcome = "rejected",
                            reason = "negative_size",
                            "protocol_stage"
                        );
                        ProtocolError::InvalidPayload
                    })?;
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "accepted",
                        event_type = "file_header",
                        id_matches_expected,
                        size_matches_expected,
                        byte_count = total_size,
                        "protocol_stage"
                    );
                    return Ok((id, size));
                }
                Event::Disconnected => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "receive",
                        outcome = "failed",
                        reason = "connection_disconnected",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::Disconnected);
                }
                _ => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "expected_file_header",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::InvalidPayload);
                }
            }
        }
    }

    pub(super) fn receive_file_chunks<Writer, Progress, Cancelled>(
        &mut self,
        id: i64,
        size: u64,
        writer: &mut Writer,
        on_progress: &mut Progress,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Writer: Write,
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        let mut received = 0_u64;
        let mut skipped_control_reported = false;
        loop {
            self.stop_if_cancelled(is_cancelled)?;
            let event = self.next_transfer_event_for(id)?;
            if let Event::Bytes { bytes, .. } = &event {
                if let Some(event_type) = frames::control_event_type(bytes) {
                    if event_type == "cancel" {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "control",
                            operation = "demux",
                            outcome = "cancelled",
                            reason = "remote",
                            event_type,
                            "protocol_stage"
                        );
                        return Err(ProtocolError::Cancelled);
                    }
                    if skipped_control_reported {
                        tracing::trace!(
                            target: "omarchy_quickshare::protocol",
                            stage = "control",
                            operation = "demux",
                            outcome = "skipped",
                            event_type,
                            "protocol_stage"
                        );
                    } else {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "control",
                            operation = "demux",
                            outcome = "skipped",
                            event_type,
                            "protocol_stage"
                        );
                        skipped_control_reported = true;
                    }
                    continue;
                }
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "control",
                    operation = "demux",
                    outcome = "rejected",
                    reason = "malformed",
                    event_type = "unknown",
                    "protocol_stage"
                );
                return Err(ProtocolError::InvalidPayload);
            }
            let Event::FileChunk {
                id: chunk_id,
                offset,
                bytes: chunk,
                is_last,
            } = event
            else {
                return if matches!(event, Event::Disconnected) {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "receive",
                        outcome = "failed",
                        reason = "connection_disconnected",
                        "protocol_stage"
                    );
                    Err(ProtocolError::Disconnected)
                } else {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "expected_file_chunk",
                        "protocol_stage"
                    );
                    Err(ProtocolError::InvalidPayload)
                };
            };
            let expected_offset = i64::try_from(received).map_err(|_| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "payload",
                    outcome = "rejected",
                    reason = "offset_out_of_range",
                    "protocol_stage"
                );
                ProtocolError::InvalidPayload
            })?;
            let chunk_size = u64::try_from(chunk.len()).map_err(|_| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "payload",
                    outcome = "rejected",
                    reason = "size_out_of_range",
                    "protocol_stage"
                );
                ProtocolError::InvalidPayload
            })?;
            let next_received =
                received.checked_add(chunk_size).ok_or_else(|| {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "size_overflow",
                        "protocol_stage"
                    );
                    ProtocolError::InvalidPayload
                })?;
            let id_matches_expected = chunk_id == id;
            let offset_matches_expected = offset == expected_offset;
            let size_matches_expected = next_received <= size;
            if !id_matches_expected
                || !offset_matches_expected
                || !size_matches_expected
                || (chunk.is_empty() && !is_last)
            {
                let reason = if !id_matches_expected {
                    "id_mismatch"
                } else if !offset_matches_expected {
                    "offset_mismatch"
                } else if !size_matches_expected {
                    "size_exceeded"
                } else {
                    "empty_nonterminal_chunk"
                };
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "payload",
                    outcome = "rejected",
                    reason,
                    id_matches_expected,
                    offset_matches_expected,
                    size_matches_expected,
                    offset,
                    byte_count = chunk_size,
                    "protocol_stage"
                );
                return Err(ProtocolError::InvalidPayload);
            }
            writer.write_all(&chunk)?;
            received = next_received;
            on_progress(received);
            tracing::trace!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "receive",
                outcome = "chunk_written",
                frame_type = "file",
                offset,
                byte_count = chunk_size,
                "protocol_stage"
            );
            if is_last {
                if received == size {
                    return Ok(());
                }
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "payload",
                    outcome = "rejected",
                    reason = "terminal_size_mismatch",
                    size_matches_expected = false,
                    byte_count = received,
                    "protocol_stage"
                );
                return Err(ProtocolError::InvalidPayload);
            }
        }
    }
}
