use crate::protocol::{
    EndpointInfo, IncomingOffer, PairingError, PairingStatus, PairingStep,
    ProtocolError, frames, offer,
};
use core::sync::atomic::{AtomicI64, Ordering};
use quickshare_connections::{Connection, ConnectionIo, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_wire::sharing::{Frame, connection_response_frame};
use rand_core::{OsRng, RngCore as _};
use std::net::TcpStream;

mod transfer;

const FILE_PAYLOAD_ID: i64 = 3;
const FILE_CHUNK_SIZE: usize = 0x0001_0000;

static NEXT_CONTROL_PAYLOAD_ID: AtomicI64 = AtomicI64::new(1);

#[must_use]
fn next_control_payload_id() -> i64 {
    loop {
        let id = NEXT_CONTROL_PAYLOAD_ID.load(Ordering::Relaxed);
        let next = match id {
            2 => 4,
            i64::MAX => 1,
            _ => id + 1,
        };
        if NEXT_CONTROL_PAYLOAD_ID
            .compare_exchange_weak(
                id,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        {
            return id;
        }
    }
}

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
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "pairing",
            operation = "exchange",
            outcome = "started",
            "protocol_stage"
        );
        self.send_control_frame(&frames::account_free_encryption())
            .map_err(|source| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "pairing",
                    operation = "send_encryption",
                    outcome = "failed",
                    "protocol_stage"
                );
                PairingError::new(PairingStep::SendEncryption, source)
            })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "pairing",
            operation = "send_encryption",
            outcome = "locally_written",
            frame_type = "paired_key_encryption",
            "protocol_stage"
        );
        let encryption = self.receive_bytes().map_err(|source| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "pairing",
                operation = "receive_encryption",
                outcome = "failed",
                "protocol_stage"
            );
            PairingError::new(PairingStep::ReceiveEncryption, source)
        })?;
        let _ = frames::decode_pairing(&encryption).map_err(|source| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "pairing",
                operation = "receive_encryption",
                outcome = "rejected",
                "protocol_stage"
            );
            PairingError::new(PairingStep::ReceiveEncryption, source)
        })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "pairing",
            operation = "receive_encryption",
            outcome = "accepted",
            frame_type = "paired_key_encryption",
            "protocol_stage"
        );
        self.send_control_frame(&frames::account_free_result())
            .map_err(|source| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "pairing",
                    operation = "send_result",
                    outcome = "failed",
                    "protocol_stage"
                );
                PairingError::new(PairingStep::SendResult, source)
            })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "pairing",
            operation = "send_result",
            outcome = "locally_written",
            frame_type = "paired_key_result",
            "protocol_stage"
        );
        let result = self.receive_bytes().map_err(|source| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "pairing",
                operation = "receive_result",
                outcome = "failed",
                "protocol_stage"
            );
            PairingError::new(PairingStep::ReceiveResult, source)
        })?;
        let status = frames::decode_pairing(&result).map_err(|source| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "pairing",
                operation = "receive_result",
                outcome = "rejected",
                "protocol_stage"
            );
            PairingError::new(PairingStep::ReceiveResult, source)
        })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "pairing",
            operation = "receive_result",
            outcome = "accepted",
            frame_type = "paired_key_result",
            "protocol_stage"
        );
        Ok(status)
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
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "introduction",
            operation = "receive",
            outcome = "started",
            "protocol_stage"
        );
        let bytes = self.receive_bytes().inspect_err(|_error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "introduction",
                operation = "receive",
                outcome = "failed",
                reason = "receive",
                "protocol_stage"
            );
        })?;
        let offer = Self::decode_offer(&bytes).inspect_err(|_error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "introduction",
                operation = "receive",
                outcome = "rejected",
                reason = "validation",
                "protocol_stage"
            );
        })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "introduction",
            operation = "receive",
            outcome = "accepted",
            frame_type = "introduction",
            byte_count = offer.size_bytes(),
            "protocol_stage"
        );
        Ok(offer)
    }

    /// Writes the standard accept response for the current inbound offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn accept_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.send_consent(&frames::accept_response(), "accepted")
    }

    /// Writes the standard rejection response for the current inbound offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn reject_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.send_consent(&frames::reject_response(), "rejected")
    }

    /// Writes the timed-out response for the current inbound offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn timeout_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.send_consent(&frames::timeout_response(), "timed_out")
    }

    /// Writes the unsupported-attachment response for the current inbound
    /// offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the response cannot be sent.
    pub fn unsupported_incoming_offer(&mut self) -> Result<(), ProtocolError> {
        self.send_consent(&frames::unsupported_response(), "unsupported")
    }

    fn send_consent(
        &mut self,
        frame: &Frame,
        reason: &'static str,
    ) -> Result<(), ProtocolError> {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "consent",
            operation = "send",
            outcome = "started",
            reason,
            "protocol_stage"
        );
        let result = self.send_control_frame(frame);
        let outcome = if result.is_ok() {
            "locally_written"
        } else {
            "failed"
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "consent",
            operation = "send",
            outcome,
            reason,
            event_type = "response",
            "protocol_stage"
        );
        result
    }

    pub(in crate::protocol) fn send_control_frame(
        &mut self,
        frame: &Frame,
    ) -> Result<(), ProtocolError> {
        self.connection
            .send_sharing_frame(next_control_payload_id(), frame)?;
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
