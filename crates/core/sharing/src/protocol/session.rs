use crate::protocol::{
    EndpointInfo, IncomingOffer, PairingStatus, ProtocolError, frames, offer,
};
use quickshare_connections::{Connection, ConnectionOptions, Event};
use quickshare_crypto::Handshake;
use quickshare_wire::sharing::{Frame, connection_response_frame};
use rand_core::{OsRng, RngCore as _};
use std::{
    io::{Read, Write},
    net::TcpStream,
};

const PAIRING_PAYLOAD_ID: i64 = 1;
const INTRODUCTION_PAYLOAD_ID: i64 = 2;
const FILE_PAYLOAD_ID: i64 = 3;
const CANCEL_PAYLOAD_ID: i64 = 4;
const FILE_CHUNK_SIZE: usize = 0x0001_0000;

/// Drives account-free Sharing over an encrypted Connections relationship.
#[derive(Debug)]
pub struct SharingSession {
    connection: Connection,
}

impl SharingSession {
    /// Establishes an encrypted outbound TCP relationship with a peer.
    ///
    /// # Errors
    ///
    /// Returns an error when the Connections or UKEY2 exchange fails.
    pub fn connect(
        stream: TcpStream,
        endpoint_id: &str,
        endpoint_name: &str,
    ) -> Result<Self, ProtocolError> {
        let mut rng = OsRng;
        let options = connection_options(&mut rng, endpoint_id, endpoint_name)?;
        let connection = Connection::connect(
            stream,
            Handshake::initiator_with_rng(&mut rng),
            options,
        )?;
        Ok(Self::new(connection))
    }

    /// Establishes an encrypted inbound TCP relationship with a peer.
    ///
    /// # Errors
    ///
    /// Returns an error when the Connections or UKEY2 exchange fails.
    pub fn accept(
        stream: TcpStream,
        endpoint_id: &str,
        endpoint_name: &str,
    ) -> Result<Self, ProtocolError> {
        let mut rng = OsRng;
        let options = connection_options(&mut rng, endpoint_id, endpoint_name)?;
        let connection = Connection::accept(
            stream,
            Handshake::responder_with_rng(&mut rng),
            options,
        )?;
        Ok(Self::new(connection))
    }

    /// Owns one encrypted Connections relationship.
    #[must_use]
    pub const fn new(connection: Connection) -> Self {
        Self { connection }
    }

    /// Returns the shared four-digit UKEY2 peer-verification code.
    #[must_use]
    pub fn verification_code(&self) -> &str {
        self.connection.verification_code()
    }

    /// Exchanges paired-key frames and verifies mutual account-free `UNABLE`.
    ///
    /// # Errors
    ///
    /// Returns an error when a connection or peer frame is invalid.
    pub fn exchange_account_free_pairing(
        &mut self,
    ) -> Result<PairingStatus, ProtocolError> {
        self.connection.send_sharing_frame(
            PAIRING_PAYLOAD_ID,
            &frames::account_free_encryption(),
        )?;
        let _ = frames::decode_pairing(&self.receive_bytes()?)?;
        self.connection.send_sharing_frame(
            PAIRING_PAYLOAD_ID + 1,
            &frames::account_free_result(),
        )?;
        frames::decode_pairing(&self.receive_bytes()?)
    }

    /// Returns the account-free paired-key result frame.
    #[must_use]
    pub fn account_free_result() -> Frame {
        frames::account_free_result()
    }

    /// Receives and validates exactly one inbound file introduction.
    ///
    /// # Errors
    ///
    /// Returns an error when the next event is not one safe file offer.
    pub fn receive_incoming_offer(
        &mut self,
    ) -> Result<IncomingOffer, ProtocolError> {
        Self::decode_offer(&self.receive_bytes()?)
    }

    /// Writes the standard accept response for the current inbound offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn accept_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.connection.send_sharing_frame(
            INTRODUCTION_PAYLOAD_ID,
            &frames::accept_response(),
        )?;
        Ok(())
    }

    /// Writes the standard rejection response for the current inbound offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn reject_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.connection.send_sharing_frame(
            INTRODUCTION_PAYLOAD_ID,
            &frames::reject_response(),
        )?;
        Ok(())
    }

    /// Receives a file payload into a caller-owned destination.
    ///
    /// # Errors
    ///
    /// Returns an error when payload metadata, chunks, lengths, or writes fail.
    pub fn receive_incoming_file<Writer, Cancelled>(
        &mut self,
        offer: &IncomingOffer,
        writer: &mut Writer,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Writer: Write,
        Cancelled: FnMut() -> bool,
    {
        self.stop_if_cancelled(&mut is_cancelled)?;
        let (id, size) = self.receive_file_header(offer)?;
        self.receive_file_chunks(id, size, writer, &mut is_cancelled)
    }

    /// Introduces one file, reports peer consent, then streams its payload.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe names, invalid lengths, peer rejection, or
    /// transfer failure. The callback runs exactly once after peer acceptance
    /// and before the first payload frame.
    pub fn send_outgoing_file<Reader, Accepted, Cancelled>(
        &mut self,
        name: &str,
        size: u64,
        reader: &mut Reader,
        on_accepted: Accepted,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Reader: Read,
        Accepted: FnOnce(),
        Cancelled: FnMut() -> bool,
    {
        if !offer::safe_name(name) {
            return Err(ProtocolError::InvalidOffer("unsafe file name"));
        }
        let wire_size =
            i64::try_from(size).map_err(|_| ProtocolError::InvalidPayload)?;
        if is_cancelled() {
            return Err(ProtocolError::Cancelled);
        }
        self.connection.send_sharing_frame(
            INTRODUCTION_PAYLOAD_ID,
            &frames::introduction(name, wire_size),
        )?;
        if Self::decode_response(&self.receive_bytes()?)?
            != connection_response_frame::Status::Accept
        {
            return Err(ProtocolError::Rejected);
        }
        on_accepted();
        self.stop_if_cancelled(&mut is_cancelled)?;
        self.send_file_payload(name, reader, size, wire_size, &mut is_cancelled)
    }

    /// Decodes one Google file introduction fixture or received Sharing frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the bytes do not describe exactly one safe file.
    pub fn decode_offer(bytes: &[u8]) -> Result<IncomingOffer, ProtocolError> {
        offer::decode(bytes)
    }

    /// Decodes one Sharing response frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the response is absent, malformed, or unknown.
    pub fn decode_response(
        bytes: &[u8],
    ) -> Result<connection_response_frame::Status, ProtocolError> {
        frames::decode_response(bytes)
    }

    fn receive_file_header(
        &mut self,
        offer: &IncomingOffer,
    ) -> Result<(i64, u64), ProtocolError> {
        let event = self.connection.receive()?;
        if let Event::Bytes { bytes, .. } = &event
            && frames::is_cancel(bytes)?
        {
            return Err(ProtocolError::Cancelled);
        }
        let Event::FileHeader {
            id,
            total_size,
            name,
        } = event
        else {
            return Err(ProtocolError::InvalidPayload);
        };
        if id != offer.payload_id()
            || total_size != offer.size_bytes()
            || name.as_deref() != Some(offer.name())
        {
            return Err(ProtocolError::InvalidPayload);
        }
        u64::try_from(total_size)
            .map(|size| (id, size))
            .map_err(|_| ProtocolError::InvalidPayload)
    }

    fn receive_file_chunks<Writer, Cancelled>(
        &mut self,
        id: i64,
        size: u64,
        writer: &mut Writer,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Writer: Write,
        Cancelled: FnMut() -> bool,
    {
        let mut received = 0_u64;
        loop {
            self.stop_if_cancelled(is_cancelled)?;
            let event = self.connection.receive()?;
            if let Event::Bytes { bytes, .. } = &event
                && frames::is_cancel(bytes)?
            {
                return Err(ProtocolError::Cancelled);
            }
            let Event::FileChunk {
                id: chunk_id,
                offset,
                bytes: chunk,
                is_last,
            } = event
            else {
                return Err(ProtocolError::InvalidPayload);
            };
            let expected_offset = i64::try_from(received)
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let chunk_size = u64::try_from(chunk.len())
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let next_received = received
                .checked_add(chunk_size)
                .ok_or(ProtocolError::InvalidPayload)?;
            if chunk_id != id
                || offset != expected_offset
                || next_received > size
                || (chunk.is_empty() && !is_last)
            {
                return Err(ProtocolError::InvalidPayload);
            }
            writer.write_all(&chunk)?;
            received = next_received;
            if is_last {
                return (received == size)
                    .then_some(())
                    .ok_or(ProtocolError::InvalidPayload);
            }
        }
    }

    fn send_file_payload<Reader, Cancelled>(
        &mut self,
        name: &str,
        reader: &mut Reader,
        size: u64,
        wire_size: i64,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Reader: Read,
        Cancelled: FnMut() -> bool,
    {
        self.connection.send_file_header(
            FILE_PAYLOAD_ID,
            wire_size,
            Some(name.into()),
        )?;
        if size == 0 {
            self.stop_if_cancelled(is_cancelled)?;
            let mut extra = [0_u8; 1];
            if reader.read(&mut extra)? != 0 {
                return Err(ProtocolError::InvalidPayload);
            }
            self.connection
                .send_file_chunk(FILE_PAYLOAD_ID, 0, &[], true)?;
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
                return Err(ProtocolError::InvalidPayload);
            }
            let read_size = u64::try_from(read)
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let next_offset = offset
                .checked_add(read_size)
                .ok_or(ProtocolError::InvalidPayload)?;
            let is_last = next_offset == size;
            if is_last {
                let mut extra = [0_u8; 1];
                if reader.read(&mut extra)? != 0 {
                    return Err(ProtocolError::InvalidPayload);
                }
            }
            self.connection.send_file_chunk(
                FILE_PAYLOAD_ID,
                i64::try_from(offset)
                    .map_err(|_| ProtocolError::InvalidPayload)?,
                &buffer[..read],
                is_last,
            )?;
            offset = next_offset;
        }
        Ok(())
    }

    fn receive_bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        match self.connection.receive()? {
            Event::Bytes { bytes, .. } if frames::is_cancel(&bytes)? => {
                Err(ProtocolError::Cancelled)
            }
            Event::Bytes { bytes, .. } => Ok(bytes),
            _ => Err(ProtocolError::InvalidFrame),
        }
    }

    fn stop_if_cancelled<Cancelled>(
        &mut self,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Cancelled: FnMut() -> bool,
    {
        if !is_cancelled() {
            return Ok(());
        }
        self.connection
            .send_sharing_frame(CANCEL_PAYLOAD_ID, &frames::cancel())?;
        Err(ProtocolError::Cancelled)
    }
}

fn connection_options(
    rng: &mut OsRng,
    endpoint_id: &str,
    endpoint_name: &str,
) -> Result<ConnectionOptions, ProtocolError> {
    let mut salt = [0; 2];
    let mut metadata_key = [0; 14];
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut metadata_key);
    let endpoint_info = EndpointInfo::new(
        0,
        5,
        salt,
        metadata_key,
        Some(endpoint_name),
        None,
        Vec::new(),
    )?
    .encode();
    Ok(ConnectionOptions::new(endpoint_id, endpoint_name)
        .with_endpoint_info(endpoint_info))
}
