use super::SharingSession;
use crate::protocol::{IncomingOffer, ProtocolError, frames, offer};
use std::io::{Read, Write};

mod control;
mod file;
mod receive;

impl SharingSession {
    /// Receives a file payload into a caller-owned destination.
    ///
    /// # Errors
    ///
    /// Returns an error when payload metadata, chunks, lengths, or writes fail.
    pub fn receive_incoming_file<Writer, Progress, Cancelled>(
        &mut self,
        offer: &IncomingOffer,
        writer: &mut Writer,
        mut on_progress: Progress,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Writer: Write,
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "receive",
            outcome = "started",
            frame_type = "file",
            byte_count = offer.size_bytes(),
            "protocol_stage"
        );
        self.stop_if_cancelled(&mut is_cancelled)?;
        let (id, size) = self.receive_file_header(offer)?;
        let result = self.receive_file_chunks(
            id,
            size,
            writer,
            &mut on_progress,
            &mut is_cancelled,
        );
        if result.is_ok() {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "receive",
                outcome = "completed",
                frame_type = "file",
                byte_count = size,
                "protocol_stage"
            );
        } else {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "receive",
                outcome = "failed",
                frame_type = "file",
                "protocol_stage"
            );
        }
        result
    }

    /// Introduces one file, reports peer consent, then streams its payload.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe names, invalid lengths, peer rejection, or
    /// transfer failure. The callback runs exactly once after peer acceptance
    /// and before the first payload frame.
    pub fn send_outgoing_file<Reader, Accepted, Progress, Cancelled>(
        &mut self,
        name: &str,
        size: u64,
        reader: &mut Reader,
        on_accepted: Accepted,
        mut on_progress: Progress,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Reader: Read,
        Accepted: FnOnce(),
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        if !offer::safe_name(name) {
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
        let wire_size = i64::try_from(size).map_err(|_| {
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
        if is_cancelled() {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "control",
                operation = "cancel",
                outcome = "cancelled",
                reason = "local",
                "protocol_stage"
            );
            return Err(ProtocolError::Cancelled);
        }
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "introduction",
            operation = "send",
            outcome = "started",
            event_type = "file",
            byte_count = size,
            "protocol_stage"
        );
        self.send_control_frame(&frames::introduction(name, wire_size))
            .inspect_err(|_error| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "introduction",
                    operation = "send",
                    outcome = "failed",
                    event_type = "file",
                    "protocol_stage"
                );
            })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "introduction",
            operation = "send",
            outcome = "locally_written",
            frame_type = "introduction",
            event_type = "file",
            byte_count = size,
            "protocol_stage"
        );
        frames::consent_result(Self::decode_response(&self.receive_bytes()?)?)?;
        on_accepted();
        self.stop_if_cancelled(&mut is_cancelled)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "send",
            outcome = "started",
            frame_type = "file",
            byte_count = size,
            "protocol_stage"
        );
        self.send_file_payload(
            name,
            reader,
            size,
            wire_size,
            &mut on_progress,
            &mut is_cancelled,
        )
        .inspect_err(|_error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "send",
                outcome = "failed",
                frame_type = "file",
                "protocol_stage"
            );
        })?;
        self.stop_if_cancelled(&mut is_cancelled)
    }
}
