use crate::protocol::{
    EndpointInfo, IncomingOffer, PairingError, PairingStatus, PairingStep,
    ProtocolError, frames, offer,
};
use quickshare_connections::{Connection, ConnectionIo, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_wire::sharing::{Frame, connection_response_frame};
use rand_core::{OsRng, RngCore as _};
use std::net::TcpStream;

mod transfer;

const PAIRING_PAYLOAD_ID: i64 = 1;
const INTRODUCTION_PAYLOAD_ID: i64 = 2;
const FILE_PAYLOAD_ID: i64 = 3;
pub(in crate::protocol) const CANCEL_PAYLOAD_ID: i64 = 4;
const FILE_CHUNK_SIZE: usize = 0x0001_0000;

/// Drives account-free Sharing over an encrypted Connections relationship.
#[derive(Debug)]
pub struct SharingSession {
    pub(in crate::protocol) connection: Connection,
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
        Self::connect_io(stream, endpoint_id, endpoint_name)
    }

    /// Establishes an encrypted outbound relationship over a byte stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the Connections or UKEY2 exchange fails.
    pub fn connect_io<Stream>(
        stream: Stream,
        endpoint_id: &str,
        endpoint_name: &str,
    ) -> Result<Self, ProtocolError>
    where
        Stream: ConnectionIo + 'static,
    {
        let mut rng = OsRng;
        let options = connection_options(&mut rng, endpoint_id, endpoint_name)?;
        let connection = Connection::connect_io(
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
        Self::accept_io(stream, endpoint_id, endpoint_name)
    }

    /// Establishes an encrypted inbound relationship over a byte stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the Connections or UKEY2 exchange fails.
    pub fn accept_io<Stream>(
        stream: Stream,
        endpoint_id: &str,
        endpoint_name: &str,
    ) -> Result<Self, ProtocolError>
    where
        Stream: ConnectionIo + 'static,
    {
        let mut rng = OsRng;
        let options = connection_options(&mut rng, endpoint_id, endpoint_name)?;
        let connection = Connection::accept_io(
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
    /// Returns an error attributed to the paired-key operation that failed.
    pub fn exchange_account_free_pairing(
        &mut self,
    ) -> Result<PairingStatus, PairingError> {
        self.connection
            .send_sharing_frame(
                PAIRING_PAYLOAD_ID,
                &frames::account_free_encryption(),
            )
            .map_err(|source| {
                PairingError::new(
                    PairingStep::SendEncryption,
                    ProtocolError::from(source),
                )
            })?;
        let encryption = self.receive_bytes().map_err(|source| {
            PairingError::new(PairingStep::ReceiveEncryption, source)
        })?;
        let _ = frames::decode_pairing(&encryption).map_err(|source| {
            PairingError::new(PairingStep::ReceiveEncryption, source)
        })?;
        self.connection
            .send_sharing_frame(
                PAIRING_PAYLOAD_ID + 1,
                &frames::account_free_result(),
            )
            .map_err(|source| {
                PairingError::new(
                    PairingStep::SendResult,
                    ProtocolError::from(source),
                )
            })?;
        let result = self.receive_bytes().map_err(|source| {
            PairingError::new(PairingStep::ReceiveResult, source)
        })?;
        frames::decode_pairing(&result).map_err(|source| {
            PairingError::new(PairingStep::ReceiveResult, source)
        })
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

    /// Writes the timed-out response for the current inbound offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn timeout_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.connection.send_sharing_frame(
            INTRODUCTION_PAYLOAD_ID,
            &frames::timeout_response(),
        )?;
        Ok(())
    }

    /// Writes the unsupported-attachment response for the current inbound
    /// offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn unsupported_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.connection.send_sharing_frame(
            INTRODUCTION_PAYLOAD_ID,
            &frames::unsupported_response(),
        )?;
        Ok(())
    }

    /// Sends a Connections keepalive request.
    ///
    /// # Errors
    ///
    /// Returns an error when encryption or transmission fails.
    pub fn send_keepalive(
        &mut self,
        sequence: u32,
    ) -> Result<(), ProtocolError> {
        self.connection.send_keepalive(sequence)?;
        Ok(())
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
        3,
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
