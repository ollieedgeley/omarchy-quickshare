use crate::protocol::{
    EndpointInfo, IncomingFile, IncomingOffer, PairingStatus, ProtocolError,
    frames, offer,
};
use quickshare_connections::{Connection, ConnectionOptions, Event};
use quickshare_crypto::Handshake;
use quickshare_wire::sharing::{Frame, connection_response_frame};
use rand_core::{OsRng, RngCore as _};
use std::net::TcpStream;

const PAIRING_PAYLOAD_ID: i64 = 1;
const INTRODUCTION_PAYLOAD_ID: i64 = 2;
const FILE_PAYLOAD_ID: i64 = 3;
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

    /// Receives a file payload without accepting unsafe names or chunks.
    ///
    /// # Errors
    ///
    /// Returns an error when payload metadata, chunks, or lengths do not match.
    pub fn receive_incoming_file(
        &mut self,
        offer: &IncomingOffer,
    ) -> Result<IncomingFile, ProtocolError> {
        let (id, size) = self.receive_file_header(offer)?;
        let bytes = self.receive_file_chunks(id, size)?;
        Ok(IncomingFile::new(offer.name().into(), bytes))
    }

    /// Introduces one file, waits for consent, then sends its complete payload.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe names, peer rejection, or transfer failure.
    pub fn send_outgoing_file(
        &mut self,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), ProtocolError> {
        if !offer::safe_name(name) {
            return Err(ProtocolError::InvalidOffer("unsafe file name"));
        }
        let size = i64::try_from(bytes.len())
            .map_err(|_| ProtocolError::InvalidPayload)?;
        self.connection.send_sharing_frame(
            INTRODUCTION_PAYLOAD_ID,
            &frames::introduction(name, size),
        )?;
        if Self::decode_response(&self.receive_bytes()?)?
            != connection_response_frame::Status::Accept
        {
            return Err(ProtocolError::Rejected);
        }
        self.send_file_payload(name, bytes, size)
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
    ) -> Result<(i64, usize), ProtocolError> {
        let Event::FileHeader {
            id,
            total_size,
            name,
        } = self.connection.receive()?
        else {
            return Err(ProtocolError::InvalidPayload);
        };
        if id != offer.payload_id()
            || total_size != offer.size_bytes()
            || name.as_deref() != Some(offer.name())
        {
            return Err(ProtocolError::InvalidPayload);
        }
        usize::try_from(total_size)
            .map(|size| (id, size))
            .map_err(|_| ProtocolError::InvalidPayload)
    }

    fn receive_file_chunks(
        &mut self,
        id: i64,
        size: usize,
    ) -> Result<Vec<u8>, ProtocolError> {
        let mut bytes = Vec::with_capacity(size);
        loop {
            let Event::FileChunk {
                id: chunk_id,
                offset,
                bytes: chunk,
                is_last,
            } = self.connection.receive()?
            else {
                return Err(ProtocolError::InvalidPayload);
            };
            let expected_offset = i64::try_from(bytes.len())
                .map_err(|_| ProtocolError::InvalidPayload)?;
            if chunk_id != id
                || offset != expected_offset
                || bytes.len().saturating_add(chunk.len()) > size
            {
                return Err(ProtocolError::InvalidPayload);
            }
            bytes.extend_from_slice(&chunk);
            if is_last {
                return (bytes.len() == size)
                    .then_some(bytes)
                    .ok_or(ProtocolError::InvalidPayload);
            }
        }
    }

    fn send_file_payload(
        &mut self,
        name: &str,
        bytes: &[u8],
        size: i64,
    ) -> Result<(), ProtocolError> {
        self.connection.send_file_header(
            FILE_PAYLOAD_ID,
            size,
            Some(name.into()),
        )?;
        if bytes.is_empty() {
            self.connection
                .send_file_chunk(FILE_PAYLOAD_ID, 0, bytes, true)?;
            return Ok(());
        }
        let mut offset = 0_i64;
        for chunk in bytes.chunks(FILE_CHUNK_SIZE) {
            let chunk_size = i64::try_from(chunk.len())
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let next_offset = offset
                .checked_add(chunk_size)
                .ok_or(ProtocolError::InvalidPayload)?;
            self.connection.send_file_chunk(
                FILE_PAYLOAD_ID,
                offset,
                chunk,
                next_offset == size,
            )?;
            offset = next_offset;
        }
        Ok(())
    }

    fn receive_bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        match self.connection.receive()? {
            Event::Bytes { bytes, .. } => Ok(bytes),
            _ => Err(ProtocolError::InvalidFrame),
        }
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
