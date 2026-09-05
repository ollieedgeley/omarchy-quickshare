use super::super::{
    Connection, Error, Event, IncomingBytes, MAX_FRAME_LENGTH, OutgoingFile,
    PayloadKind, frame::payload_header_data,
};
use super::{
    connection_event, connection_received, frame_dispatch_rejected,
    frame_rejected,
    frames::{data, decoded, disconnect, keepalive, payload_header},
    keepalive_received, keepalive_sent, payload_ack_received,
    payload_chunk_received, payload_chunk_sent, payload_control_dispatched,
    payload_debug_progress, payload_debug_progress_at, payload_trace_progress,
    upgrade_frame_rejected,
};
use prost::Message as _;
use quickshare_wire::{
    connections::{
        BandwidthUpgradeNegotiationFrame, KeepAliveFrame, OfflineFrame,
        PayloadTransferFrame, V1Frame,
        payload_transfer_frame::{
            self, control_message::EventType as ControlEvent,
        },
        v1_frame,
    },
    sharing::Frame as SharingFrame,
};
use std::io as std_io;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Connection methods split handshake, transfer, and upgrade"
)]
impl Connection {
    /// Sends a Sharing protobuf as one complete BYTES payload.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the payload is too large or transmission fails.
    pub fn send_sharing_frame(
        &mut self,
        id: i64,
        frame: &SharingFrame,
    ) -> Result<(), Error> {
        self.send_bytes(id, &frame.encode_to_vec())
    }
    /// Sends one complete BYTES payload.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the payload is too large or transmission fails.
    pub fn send_bytes(&mut self, id: i64, bytes: &[u8]) -> Result<(), Error> {
        let size = i64::try_from(bytes.len()).map_err(|_| {
            payload_rejected("send", "size_out_of_bounds");
            Error::InvalidPayload
        })?;
        let header = payload_header(id, PayloadKind::Bytes, size, None)?;
        payload_trace_progress("send", "started", "bytes", size);
        if bytes.is_empty() {
            self.data(header, 0, bytes, true)?;
        } else {
            self.data(header.clone(), 0, bytes, false)?;
            self.data(header, size, &[], true)?;
        }
        payload_trace_progress("send", "locally_written", "bytes", size);
        Ok(())
    }
    /// Records a FILE payload declaration for its subsequent DATA chunks.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for an invalid declared size or failed transmission.
    pub fn send_file_header(
        &mut self,
        id: i64,
        size: i64,
        name: Option<String>,
    ) -> Result<(), Error> {
        self.outgoing_file = Some(OutgoingFile {
            id,
            header: payload_header(id, PayloadKind::File, size, name)?,
            started: false,
        });
        payload_debug_progress("send", "started", "file", size);
        Ok(())
    }
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when no matching file is active or transmission fails.
    pub fn send_file_chunk(
        &mut self,
        id: i64,
        offset: i64,
        bytes: &[u8],
        last: bool,
    ) -> Result<(), Error> {
        let Some(file) = self.outgoing_file.as_ref() else {
            payload_rejected("send", "missing_file_header");
            return Err(Error::InvalidPayload);
        };
        if file.id != id {
            payload_rejected("send", "id_mismatch");
            return Err(Error::InvalidPayload);
        }
        if file.started {
            self.poll_outgoing_control(id)?;
        }
        let header = self
            .outgoing_file
            .as_ref()
            .ok_or(Error::InvalidPayload)?
            .header
            .clone();
        self.data(header, offset, bytes, last)?;
        payload_chunk_sent(offset, bytes.len());
        if last {
            self.outgoing_file = None;
            payload_debug_progress_at(
                "send",
                "locally_written",
                "file",
                offset,
                bytes.len(),
            );
            return Ok(());
        }
        self.outgoing_file
            .as_mut()
            .ok_or(Error::InvalidPayload)?
            .started = true;
        Ok(())
    }
    /// Sends a keepalive request with `sequence`.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when encryption or transmission fails.
    pub fn send_keepalive(&mut self, sequence: u32) -> Result<(), Error> {
        self.send(&keepalive(false, sequence))?;
        keepalive_sent(false);
        Ok(())
    }
    /// Receives an encrypted event and acknowledges keepalive requests.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for an invalid or unauthenticated peer frame.
    pub fn receive(&mut self) -> Result<Event, Error> {
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Ok(event);
            }
            let frame = match self.recv() {
                Err(Error::Io(error))
                    if error.kind() == std_io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(Event::Disconnected);
                }
                other => other?,
            };
            if let Some(event) = self.event(frame)? {
                return Ok(event);
            }
        }
    }
    /// Sends a disconnection frame and closes the local write half.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for failed encryption, transmission, or shutdown.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        self.send(&disconnect())?;
        connection_event("disconnect_frame", "local");
        self.stream.shutdown_write().inspect_err(|error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "control",
                operation = "shutdown_write",
                outcome = "rejected",
                reason = "io",
                io_error_kind = super::io::io_error_kind(error.kind()),
                "connection close rejected"
            );
        })?;
        Ok(())
    }

    fn data(
        &mut self,
        header: payload_transfer_frame::PayloadHeader,
        offset: i64,
        bytes: &[u8],
        last: bool,
    ) -> Result<(), Error> {
        if offset < 0 {
            payload_rejected("send", "invalid_offset");
            return Err(Error::InvalidPayload);
        }
        if bytes.len() > MAX_FRAME_LENGTH {
            payload_rejected("send", "chunk_too_large");
            return Err(Error::InvalidPayload);
        }
        self.send(&data(header, offset, bytes, last))
    }
    pub(super) fn payload(
        &mut self,
        transfer: PayloadTransferFrame,
    ) -> Result<Option<Event>, Error> {
        if transfer.packet_type
            == Some(payload_transfer_frame::PacketType::PayloadAck as i32)
        {
            let header = transfer.payload_header.ok_or_else(|| {
                payload_rejected("receive", "missing_header");
                Error::InvalidPayload
            })?;
            let _id = header.id.ok_or_else(|| {
                payload_rejected("receive", "missing_id");
                Error::InvalidPayload
            })?;
            payload_ack_received();
            return Ok(None);
        }
        if transfer.packet_type
            == Some(payload_transfer_frame::PacketType::Control as i32)
        {
            return self.control_payload(transfer);
        }
        if transfer.packet_type
            != Some(payload_transfer_frame::PacketType::Data as i32)
        {
            payload_rejected("receive", "unexpected_packet_type");
            return Err(Error::UnexpectedFrame);
        }
        let header = transfer.payload_header.ok_or_else(|| {
            payload_rejected("receive", "missing_header");
            Error::InvalidPayload
        })?;
        let (id, size, kind) = payload_header_data(&header)?;
        let chunk = transfer.payload_chunk.ok_or_else(|| {
            payload_rejected("receive", "missing_chunk");
            Error::InvalidPayload
        })?;
        let (offset, bytes, last) = decoded(chunk)?;
        payload_chunk_received(offset, bytes.len());
        if kind == PayloadKind::Bytes {
            return self.bytes_payload(id, size, offset, &bytes, last);
        }
        self.file_payload(id, size, header.file_name, offset, bytes, last)
            .map(Some)
    }
    fn control_payload(
        &mut self,
        transfer: PayloadTransferFrame,
    ) -> Result<Option<Event>, Error> {
        let header = transfer.payload_header.ok_or_else(|| {
            payload_rejected("receive", "missing_header");
            Error::InvalidPayload
        })?;
        let id = header.id.ok_or_else(|| {
            payload_rejected("receive", "missing_id");
            Error::InvalidPayload
        })?;
        let control = transfer.control_message.ok_or_else(|| {
            payload_rejected("receive", "missing_control");
            Error::InvalidPayload
        })?;
        let offset = control.offset.ok_or_else(|| {
            payload_rejected("receive", "missing_offset");
            Error::InvalidPayload
        })?;
        let total_size = header.total_size.ok_or_else(|| {
            payload_rejected("receive", "missing_total_size");
            Error::InvalidPayload
        })?;
        if offset < 0
            || total_size < -1
            || (total_size != -1 && offset > total_size)
        {
            payload_rejected("receive", "invalid_control_offset");
            return Err(Error::InvalidPayload);
        }
        let wire_event = control.event.ok_or_else(|| {
            payload_rejected("receive", "missing_control_event");
            Error::UnexpectedFrame
        })?;
        self.payload_control_event(id, offset, wire_event)
    }
    fn payload_control_event(
        &mut self,
        id: i64,
        offset: i64,
        wire_event: i32,
    ) -> Result<Option<Event>, Error> {
        let event = if wire_event == ControlEvent::PayloadError as i32 {
            payload_control_dispatched("payload_error", offset);
            Event::PayloadError { id, offset }
        } else if wire_event == ControlEvent::PayloadCanceled as i32 {
            payload_control_dispatched("payload_cancelled", offset);
            Event::PayloadCancelled { id, offset }
        } else {
            #[expect(
                deprecated,
                reason = "Generated wire still defines PayloadReceivedAck"
            )]
            if wire_event == ControlEvent::PayloadReceivedAck as i32 {
                payload_control_dispatched("payload_received_ack", offset);
                return Ok(None);
            }
            payload_rejected("receive", "unexpected_control_event");
            return Err(Error::UnexpectedFrame);
        };
        self.clear_payload(id);
        Ok(Some(event))
    }

    fn clear_payload(&mut self, id: i64) {
        drop(self.incoming_bytes.remove(&id));
        let _ = self.incoming_files.remove(&id);
    }

    fn bytes_payload(
        &mut self,
        id: i64,
        size: i64,
        offset: i64,
        bytes: &[u8],
        last: bool,
    ) -> Result<Option<Event>, Error> {
        let first_chunk = !self.incoming_bytes.contains_key(&id);
        let payload =
            self.incoming_bytes
                .entry(id)
                .or_insert_with(|| IncomingBytes {
                    bytes: Vec::new(),
                    next_offset: 0,
                    size,
                });
        let chunk_size = i64::try_from(bytes.len()).map_err(|_| {
            payload_rejected("receive", "chunk_too_large");
            Error::InvalidPayload
        })?;
        let end = offset.checked_add(chunk_size).ok_or_else(|| {
            payload_rejected("receive", "offset_overflow");
            Error::InvalidPayload
        })?;
        if payload.size != size {
            payload_rejected("receive", "size_mismatch");
            return Err(Error::InvalidPayload);
        }
        if payload.next_offset != offset {
            payload_rejected("receive", "offset_mismatch");
            return Err(Error::InvalidPayload);
        }
        if end > size {
            payload_rejected("receive", "data_past_declared_size");
            return Err(Error::InvalidPayload);
        }
        payload.bytes.extend_from_slice(bytes);
        payload.next_offset = end;
        if first_chunk {
            payload_trace_progress("receive", "started", "bytes", chunk_size);
        }
        if !last {
            return Ok(None);
        }
        self.finish_bytes_payload(id)
    }

    pub(super) fn event(
        &mut self,
        frame: OfflineFrame,
    ) -> Result<Option<Event>, Error> {
        let v1 = frame.v1.ok_or_else(|| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "frame_dispatch",
                operation = "receive",
                outcome = "rejected",
                reason = "missing_v1",
                "connection frame rejected"
            );
            Error::UnexpectedFrame
        })?;
        self.v1_event(v1)
    }

    fn v1_event(&mut self, v1: V1Frame) -> Result<Option<Event>, Error> {
        match v1.r#type {
            Some(value)
                if value == v1_frame::FrameType::Disconnection as i32 =>
            {
                connection_received();
                Ok(Some(Event::Disconnected))
            }
            Some(value) if value == v1_frame::FrameType::KeepAlive as i32 => {
                self.keepalive_event(v1.keep_alive)
            }
            Some(value)
                if value == v1_frame::FrameType::PayloadTransfer as i32 =>
            {
                let payload = v1.payload_transfer.ok_or_else(|| {
                    payload_rejected("receive", "missing_payload_transfer");
                    Error::UnexpectedFrame
                })?;
                self.payload(payload)
            }
            Some(value)
                if value
                    == v1_frame::FrameType::BandwidthUpgradeNegotiation
                        as i32 =>
            {
                self.negotiation_event(v1.bandwidth_upgrade_negotiation)
            }
            Some(value)
                if matches!(
                    v1_frame::FrameType::try_from(value),
                    Ok(v1_frame::FrameType::PairedKeyEncryption
                        | v1_frame::FrameType::AuthenticationMessage
                        | v1_frame::FrameType::AuthenticationResult
                        | v1_frame::FrameType::AutoResume
                        | v1_frame::FrameType::AutoReconnect
                        | v1_frame::FrameType::BandwidthUpgradeRetry)
                ) =>
            {
                super::frame_dispatch_ignored(value);
                Ok(None)
            }
            _ => {
                frame_dispatch_rejected("unexpected_frame_type", v1.r#type);
                Err(Error::UnexpectedFrame)
            }
        }
    }
    fn negotiation_event(
        &mut self,
        frame: Option<BandwidthUpgradeNegotiationFrame>,
    ) -> Result<Option<Event>, Error> {
        let negotiation = frame.ok_or_else(|| {
            upgrade_frame_rejected("missing_negotiation");
            Error::UnexpectedFrame
        })?;
        self.upgrade_event(negotiation)
    }

    fn keepalive_event(
        &mut self,
        frame: Option<KeepAliveFrame>,
    ) -> Result<Option<Event>, Error> {
        let frame_data = frame.ok_or_else(|| {
            frame_rejected("control", "missing_keepalive", "keepalive");
            Error::UnexpectedFrame
        })?;
        let ack = frame_data.ack.unwrap_or(false);
        let sequence = frame_data.seq_num.unwrap_or(0);
        keepalive_received();
        if !ack {
            self.send(&keepalive(true, sequence))?;
            keepalive_sent(true);
        }
        Ok(Some(Event::KeepAlive { ack, sequence }))
    }
    fn finish_bytes_payload(
        &mut self,
        id: i64,
    ) -> Result<Option<Event>, Error> {
        let completed_payload =
            self.incoming_bytes.remove(&id).ok_or_else(|| {
                payload_rejected("receive", "missing_reassembly");
                Error::InvalidPayload
            })?;
        if completed_payload.next_offset != completed_payload.size {
            payload_rejected("receive", "incomplete_payload");
            return Err(Error::InvalidPayload);
        }
        payload_trace_progress(
            "receive",
            "completed",
            "bytes",
            completed_payload.size,
        );
        Ok(Some(Event::Bytes {
            id,
            bytes: completed_payload.bytes,
        }))
    }
}

pub(super) fn payload_rejected(operation: &'static str, reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "payload",
        operation,
        outcome = "rejected",
        reason,
        "payload rejected"
    );
}
