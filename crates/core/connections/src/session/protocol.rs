use self::{
    frames::{
        data, decoded, disconnect, keepalive, payload_header, request,
        request_data, response, response_data,
    },
    io::{iv, read, receive_plain, send_plain, write},
};
use super::frame::payload_header_data;
use super::{
    Connection, ConnectionOptions, Error, Event, IncomingBytes,
    MAX_FRAME_LENGTH, OutgoingFile, PayloadKind,
};
use prost::Message as _;
use quickshare_crypto::{CompletedHandshake, Handshake};
use quickshare_wire::{
    connections::{
        OfflineFrame, PayloadTransferFrame, payload_transfer_frame, v1_frame,
    },
    sharing::Frame as SharingFrame,
};
use std::{
    collections::{HashMap, VecDeque},
    io as std_io,
    net::{Shutdown, TcpStream},
};

mod frames;
mod io;

impl Connection {
    /// Establishes encryption for the initiating side of a TCP connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when framing, UKEY2, or the peer's response fails.
    pub fn connect(
        mut stream: TcpStream,
        mut handshake: Handshake,
        options: ConnectionOptions,
    ) -> Result<Self, Error> {
        send_plain(&mut stream, &request(options))?;
        write(
            &mut stream,
            &handshake.next_message().map_err(|_| Error::Handshake)?,
        )?;
        handshake
            .receive(&read(&mut stream)?)
            .map_err(|_| Error::Handshake)?;
        write(
            &mut stream,
            &handshake.next_message().map_err(|_| Error::Handshake)?,
        )?;
        send_plain(&mut stream, &response())?;
        response_data(receive_plain(&mut stream)?)?;
        Ok(Self::new(
            stream,
            handshake.complete().map_err(|_| Error::Handshake)?,
        ))
    }

    /// Establishes encryption for the accepting side of a TCP connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when framing, UKEY2, or the peer's request fails.
    pub fn accept(
        mut stream: TcpStream,
        mut handshake: Handshake,
        _options: ConnectionOptions,
    ) -> Result<Self, Error> {
        request_data(receive_plain(&mut stream)?)?;
        handshake
            .receive(&read(&mut stream)?)
            .map_err(|_| Error::Handshake)?;
        write(
            &mut stream,
            &handshake.next_message().map_err(|_| Error::Handshake)?,
        )?;
        handshake
            .receive(&read(&mut stream)?)
            .map_err(|_| Error::Handshake)?;
        send_plain(&mut stream, &response())?;
        response_data(receive_plain(&mut stream)?)?;
        Ok(Self::new(
            stream,
            handshake.complete().map_err(|_| Error::Handshake)?,
        ))
    }

    /// Returns the shared four-digit UKEY2 peer-verification code.
    #[must_use]
    pub fn verification_code(&self) -> &str {
        &self.verification_code
    }

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
        self.data(
            payload_header(id, PayloadKind::Bytes, size, None)?,
            0,
            bytes,
            true,
        )
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
    /// Sends a disconnection frame and closes the local TCP half.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] for failed encryption, transmission, or shutdown.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        self.send(&disconnect())?;
        self.stream.shutdown(Shutdown::Write)?;
        Ok(())
    }

    fn new(stream: TcpStream, completed: CompletedHandshake) -> Self {
        let verification_code =
            verification_code(completed.authentication_token());
        let channel = completed.into_channel();
        Self {
            stream,
            channel,
            incoming_bytes: HashMap::default(),
            payloads: HashMap::default(),
            incoming_file: None,
            outgoing_file: None,
            pending_events: VecDeque::default(),
            verification_code,
        }
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
    fn send(&mut self, frame: &OfflineFrame) -> Result<(), Error> {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), iv())
            .map_err(|_| Error::Crypto)?;
        write(&mut self.stream, &encrypted)
    }
    fn recv(&mut self) -> Result<OfflineFrame, Error> {
        let bytes = self
            .channel
            .decrypt(&read(&mut self.stream)?)
            .map_err(|_| Error::Crypto)?;
        Ok(OfflineFrame::decode(bytes.as_slice())?)
    }
    fn payload(
        &mut self,
        transfer: PayloadTransferFrame,
    ) -> Result<Option<Event>, Error> {
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

/// Matches Nearby Connections' decimal rendering of its raw UKEY2 token.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division_remainder_used,
    clippy::modulo_arithmetic,
    reason = "Pinned Google hash requires bounded signed C++ remainder"
)]
fn verification_code(authentication_token: &[u8; 32]) -> String {
    const HASH_MODULUS: i32 = 9_973;
    const HASH_MULTIPLIER: i32 = 31;

    let mut hash = 0_i32;
    let mut multiplier = 1_i32;
    for byte in authentication_token {
        let signed = i8::from_be_bytes([*byte]);
        hash = (hash + i32::from(signed) * multiplier) % HASH_MODULUS;
        multiplier = multiplier * HASH_MULTIPLIER % HASH_MODULUS;
    }
    format!("{:04}", hash.abs())
}
