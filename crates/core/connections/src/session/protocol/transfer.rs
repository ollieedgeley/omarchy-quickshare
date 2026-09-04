use super::super::{
    Connection, Error, Event, IncomingBytes, MAX_FRAME_LENGTH, OutgoingFile,
    PayloadKind, frame::payload_header_data,
};
use super::frames::{data, decoded, disconnect, keepalive, payload_header};
use prost::Message as _;
use quickshare_wire::{
    connections::{
        OfflineFrame, PayloadTransferFrame, payload_transfer_frame, v1_frame,
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
        let size =
            i64::try_from(bytes.len()).map_err(|_| Error::InvalidPayload)?;
        let header = payload_header(id, PayloadKind::Bytes, size, None)?;
        if bytes.is_empty() {
            return self.data(header, 0, bytes, true);
        }
        self.data(header.clone(), 0, bytes, false)?;
        self.data(header, size, &[], true)
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
        });
        Ok(())
    }
    /// Sends one FILE payload chunk.
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
            return Err(Error::InvalidPayload);
        };
        if file.id != id {
            return Err(Error::InvalidPayload);
        }
        self.data(file.header.clone(), offset, bytes, last)?;
        if last {
            self.outgoing_file = None;
        }
        Ok(())
    }
    /// Sends a keepalive request with `sequence`.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when encryption or transmission fails.
    pub fn send_keepalive(&mut self, sequence: u32) -> Result<(), Error> {
        self.send(&keepalive(false, sequence))
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
        self.stream.shutdown_write()?;
        Ok(())
    }

    fn data(
        &mut self,
        header: payload_transfer_frame::PayloadHeader,
        offset: i64,
        bytes: &[u8],
        last: bool,
    ) -> Result<(), Error> {
        if offset < 0 || bytes.len() > MAX_FRAME_LENGTH {
            return Err(Error::InvalidPayload);
        }
        self.send(&data(header, offset, bytes, last))
    }
    fn payload(
        &mut self,
        transfer: PayloadTransferFrame,
    ) -> Result<Option<Event>, Error> {
        if transfer.packet_type
            == Some(payload_transfer_frame::PacketType::PayloadAck as i32)
        {
            let header =
                transfer.payload_header.ok_or(Error::InvalidPayload)?;
            let _id = header.id.ok_or(Error::InvalidPayload)?;
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
            return Err(Error::UnexpectedFrame);
        }
        let header = transfer.payload_header.ok_or(Error::InvalidPayload)?;
        let (id, size, kind) = payload_header_data(&header)?;
        let chunk = transfer.payload_chunk.ok_or(Error::InvalidPayload)?;
        let (offset, bytes, last) = decoded(chunk)?;
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
        let header = transfer.payload_header.ok_or(Error::InvalidPayload)?;
        let id = header.id.ok_or(Error::InvalidPayload)?;
        let control = transfer.control_message.ok_or(Error::InvalidPayload)?;
        let offset = control.offset.ok_or(Error::InvalidPayload)?;
        let wire_event = control.event.ok_or(Error::UnexpectedFrame)?;
        let event = if wire_event
            == payload_transfer_frame::control_message::EventType::PayloadError
                as i32
        {
            Event::PayloadError { id, offset }
        } else if wire_event
            == payload_transfer_frame::control_message::EventType::
                PayloadCanceled as i32
        {
            Event::PayloadCancelled { id, offset }
        } else {
            #[expect(
                deprecated,
                reason = "Generated wire still defines PayloadReceivedAck"
            )]
            if wire_event
                == payload_transfer_frame::control_message::EventType::
                    PayloadReceivedAck as i32
            {
                return Ok(None);
            }
            return Err(Error::UnexpectedFrame);
        };
        drop(self.incoming_bytes.remove(&id));
        let _ = self.payloads.remove(&id);
        if self.incoming_file == Some(id) {
            self.incoming_file = None;
        }
        Ok(Some(event))
    }

    fn bytes_payload(
        &mut self,
        id: i64,
        size: i64,
        offset: i64,
        bytes: &[u8],
        last: bool,
    ) -> Result<Option<Event>, Error> {
        let payload =
            self.incoming_bytes
                .entry(id)
                .or_insert_with(|| IncomingBytes {
                    bytes: Vec::new(),
                    next_offset: 0,
                    size,
                });
        let chunk_size =
            i64::try_from(bytes.len()).map_err(|_| Error::InvalidPayload)?;
        let end = offset
            .checked_add(chunk_size)
            .ok_or(Error::InvalidPayload)?;
        if payload.size != size || payload.next_offset != offset || end > size {
            return Err(Error::InvalidPayload);
        }
        payload.bytes.extend_from_slice(bytes);
        payload.next_offset = end;
        if !last {
            return Ok(None);
        }
        let completed_payload = self
            .incoming_bytes
            .remove(&id)
            .ok_or(Error::InvalidPayload)?;
        (completed_payload.next_offset == completed_payload.size)
            .then_some(Event::Bytes {
                id,
                bytes: completed_payload.bytes,
            })
            .ok_or(Error::InvalidPayload)
            .map(Some)
    }

    fn event(&mut self, frame: OfflineFrame) -> Result<Option<Event>, Error> {
        let v1 = frame.v1.ok_or(Error::UnexpectedFrame)?;
        match v1.r#type {
            Some(value)
                if value == v1_frame::FrameType::Disconnection as i32 =>
            {
                Ok(Some(Event::Disconnected))
            }
            Some(value) if value == v1_frame::FrameType::KeepAlive as i32 => {
                let frame_data = v1.keep_alive.ok_or(Error::UnexpectedFrame)?;
                let ack = frame_data.ack.unwrap_or(false);
                let sequence = frame_data.seq_num.unwrap_or(0);
                if !ack {
                    self.send(&keepalive(true, sequence))?;
                }
                Ok(Some(Event::KeepAlive { ack, sequence }))
            }
            Some(value)
                if value == v1_frame::FrameType::PayloadTransfer as i32 =>
            {
                self.payload(v1.payload_transfer.ok_or(Error::UnexpectedFrame)?)
            }
            Some(value)
                if value
                    == v1_frame::FrameType::BandwidthUpgradeNegotiation
                        as i32 =>
            {
                self.upgrade_event(
                    v1.bandwidth_upgrade_negotiation
                        .ok_or(Error::UnexpectedFrame)?,
                )
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }
    fn file_payload(
        &mut self,
        id: i64,
        size: i64,
        name: Option<String>,
        offset: i64,
        bytes: Vec<u8>,
        last: bool,
    ) -> Result<Event, Error> {
        let first_chunk = match self.payloads.get(&id) {
            None => {
                if self.incoming_file.is_some() {
                    return Err(Error::InvalidPayload);
                }
                let _ = self.payloads.insert(id, PayloadKind::File);
                self.incoming_file = Some(id);
                true
            }
            Some(PayloadKind::File) if self.incoming_file == Some(id) => false,
            _ => return Err(Error::InvalidPayload),
        };
        let event = Event::FileChunk {
            id,
            offset,
            bytes,
            is_last: last,
        };
        if last {
            let _ = self.payloads.remove(&id);
            self.incoming_file = None;
        }
        if first_chunk {
            self.pending_events.push_back(event);
            return Ok(Event::FileHeader {
                id,
                total_size: size,
                name,
            });
        }
        Ok(event)
    }
}
