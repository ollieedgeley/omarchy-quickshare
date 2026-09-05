use super::SharingSession;
use crate::protocol::ProtocolError;
use crate::protocol::session::{FILE_CHUNK_SIZE, FILE_PAYLOAD_ID};
use std::io::Read;

impl SharingSession {
    pub(super) fn send_file_payload<Reader, Progress, Cancelled>(
        &mut self,
        name: &str,
        reader: &mut Reader,
        size: u64,
        wire_size: i64,
        on_progress: &mut Progress,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Reader: Read,
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.connection.send_file_header(
            FILE_PAYLOAD_ID,
            wire_size,
            Some(name.into()),
        )?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "send",
            outcome = "staged",
            frame_type = "file",
            byte_count = size,
            "protocol_stage"
        );
        if size == 0 {
            self.stop_if_cancelled(is_cancelled)?;
            let mut extra = [0_u8; 1];
            if reader.read(&mut extra)? != 0 {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "payload",
                    outcome = "rejected",
                    reason = "source_larger_than_declared",
                    size_matches_expected = false,
                    "protocol_stage"
                );
                return Err(ProtocolError::InvalidPayload);
            }
            self.connection
                .send_file_chunk(FILE_PAYLOAD_ID, 0, &[], true)?;
            on_progress(0);
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "terminal_marker",
                outcome = "locally_written",
                frame_type = "file",
                offset = 0,
                byte_count = 0,
                "protocol_stage"
            );
            return Ok(());
        }
        let mut buffer = vec![0_u8; FILE_CHUNK_SIZE];
        let mut offset = 0_u64;
        while offset < size {
            self.stop_if_cancelled(is_cancelled)?;
            let remaining = size - offset;
            let read_limit = usize::try_from(remaining)
                .unwrap_or(FILE_CHUNK_SIZE)
                .min(FILE_CHUNK_SIZE);
            let read = reader.read(&mut buffer[..read_limit])?;
            if read == 0 {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "payload",
                    outcome = "rejected",
                    reason = "source_shorter_than_declared",
                    size_matches_expected = false,
                    byte_count = offset,
                    "protocol_stage"
                );
                return Err(ProtocolError::InvalidPayload);
            }
            let read_size = u64::try_from(read).map_err(|_| {
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
            let next_offset =
                offset.checked_add(read_size).ok_or_else(|| {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "offset_overflow",
                        "protocol_stage"
                    );
                    ProtocolError::InvalidPayload
                })?;
            if next_offset == size {
                let mut extra = [0_u8; 1];
                if reader.read(&mut extra)? != 0 {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "source_larger_than_declared",
                        size_matches_expected = false,
                        byte_count = next_offset,
                        "protocol_stage"
                    );
                    return Err(ProtocolError::InvalidPayload);
                }
            }
            let wire_offset = i64::try_from(offset).map_err(|_| {
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
            self.connection.send_file_chunk(
                FILE_PAYLOAD_ID,
                wire_offset,
                &buffer[..read],
                false,
            )?;
            tracing::trace!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "send",
                outcome = "chunk_locally_written",
                frame_type = "file",
                offset,
                byte_count = read_size,
                "protocol_stage"
            );
            offset = next_offset;
            on_progress(offset);
        }
        self.stop_if_cancelled(is_cancelled)?;
        self.connection.send_file_chunk(
            FILE_PAYLOAD_ID,
            wire_size,
            &[],
            true,
        )?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "terminal_marker",
            outcome = "locally_written",
            frame_type = "file",
            offset = wire_size,
            byte_count = 0,
            "protocol_stage"
        );
        Ok(())
    }
}
