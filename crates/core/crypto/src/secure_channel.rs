use core::{cell::Cell, fmt};
use hmac::{Hmac, Mac as _};
use prost::Message as _;
use quickshare_wire::{
    secure_gcm as securegcm, secure_message as securemessage,
};
use sha2::{Digest as _, Sha256};

use crate::primitives::{decrypt, encrypt, hmac, java_hash, keys};
use crate::{CryptoError, IV_LENGTH, KEY_LENGTH};

const D2D_SALT: &[u8] = b"D2D";

/// A directional AES-256-CBC/HMAC-SHA256 D2D channel.
pub struct SecureChannel {
    send: Keys,
    receive: Keys,
    send_sequence: i32,
    receive_sequence: i32,
    _not_copy: Cell<()>,
}

#[derive(Clone, Copy)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) struct Keys {
    pub(super) encryption: [u8; KEY_LENGTH],
    pub(super) signing: [u8; KEY_LENGTH],
    pub(super) master: [u8; KEY_LENGTH],
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Keys").finish_non_exhaustive()
    }
}

impl fmt::Debug for SecureChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecureChannel")
            .field("send_sequence", &self.send_sequence)
            .field("receive_sequence", &self.receive_sequence)
            .finish_non_exhaustive()
    }
}

impl SecureChannel {
    pub(super) fn new(
        send: [u8; KEY_LENGTH],
        receive: [u8; KEY_LENGTH],
    ) -> Self {
        Self {
            send: keys(send),
            receive: keys(receive),
            send_sequence: 0,
            receive_sequence: 0,
            _not_copy: Cell::new(()),
        }
    }

    /// Encrypts one payload with the supplied fresh CBC IV.
    ///
    /// # Errors
    ///
    /// Returns an error if the sequence number is exhausted or encryption
    /// fails.
    pub fn encrypt(
        &mut self,
        plaintext: &[u8],
        iv: [u8; IV_LENGTH],
    ) -> Result<Vec<u8>, CryptoError> {
        self.send_sequence =
            self.send_sequence.checked_add(1).ok_or_else(|| {
                rejected("encrypt", "sequence_exhausted");
                CryptoError::SequenceExhausted
            })?;
        let payload = securegcm::DeviceToDeviceMessage {
            message: Some(plaintext.to_vec()),
            sequence_number: Some(self.send_sequence),
        }
        .encode_to_vec();
        let metadata = securegcm::GcmMetadata {
            r#type: securegcm::Type::DeviceToDeviceMessage as i32,
            version: Some(1_i32),
        }
        .encode_to_vec();
        let header = securemessage::Header {
            signature_scheme: securemessage::SigScheme::HmacSha256 as i32,
            encryption_scheme: securemessage::EncScheme::Aes256Cbc as i32,
            verification_key_id: None,
            decryption_key_id: None,
            iv: Some(iv.to_vec()),
            public_metadata: Some(metadata),
            associated_data_length: None,
        };
        let raw_header = header.encode_to_vec();
        let body = encrypt(&self.send.encryption, &iv, &payload)
            .inspect_err(|_| rejected("encrypt", "encryption"))?;
        let header_and_body = securemessage::HeaderAndBodyInternal {
            header: raw_header,
            body,
        }
        .encode_to_vec();
        let signature = hmac(&self.send.signing, &header_and_body);
        let encrypted = securemessage::SecureMessage {
            header_and_body,
            signature: signature.into(),
        }
        .encode_to_vec();
        tracing::trace!(
            target: "omarchy_quickshare::protocol",
            stage = "d2d",
            operation = "encrypt",
            outcome = "completed",
            byte_count = plaintext.len(),
            "D2D frame encrypted"
        );
        Ok(encrypted)
    }

    /// Verifies, decrypts, and sequence-checks one peer payload.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, unauthenticated, out-of-order, or
    /// invalid frames.
    pub fn decrypt(&mut self, message: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let message = securemessage::SecureMessage::decode(message)
            .map_err(|_| invalid_message("invalid_message"))?;
        let bytes = message.header_and_body.as_slice();
        let signature = message.signature.as_slice();
        let mut mac = Hmac::<Sha256>::new_from_slice(&self.receive.signing)
            .map_err(|_| authentication())?;
        mac.update(bytes);
        mac.verify_slice(signature).map_err(|_| authentication())?;
        let inner = securemessage::HeaderAndBodyInternal::decode(bytes)
            .map_err(|_| invalid_message("invalid_message"))?;
        let header = securemessage::Header::decode(inner.header.as_slice())
            .map_err(|_| invalid_message("invalid_header"))?;
        validate_header(&header)?;
        let iv: [u8; IV_LENGTH] = header
            .iv
            .as_deref()
            .ok_or_else(|| invalid_message("missing_iv"))?
            .try_into()
            .map_err(|_| invalid_message("invalid_iv"))?;
        let plain = decrypt(&self.receive.encryption, &iv, &inner.body)
            .inspect_err(|_| rejected("decrypt", "decryption"))?;
        let payload = self.decode_payload(&plain)?;
        tracing::trace!(
            target: "omarchy_quickshare::protocol",
            stage = "d2d",
            operation = "decrypt",
            outcome = "completed",
            byte_count = payload.len(),
            "D2D frame decrypted"
        );
        Ok(payload)
    }

    /// Returns the symmetric D2D session identifier.
    #[must_use]
    pub fn session_unique(&self) -> [u8; KEY_LENGTH] {
        let (first, second) =
            if java_hash(&self.send.master) < java_hash(&self.receive.master) {
                (&self.send.master, &self.receive.master)
            } else {
                (&self.receive.master, &self.send.master)
            };
        let mut digest = Sha256::new();
        digest.update(Sha256::digest(D2D_SALT));
        digest.update(first);
        digest.update(second);
        digest.finalize().into()
    }

    fn decode_payload(&mut self, plain: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let packet =
            securegcm::DeviceToDeviceMessage::decode(plain).map_err(|_| {
                rejected("decrypt", "invalid_payload");
                CryptoError::InvalidMessage
            })?;
        let expected =
            self.receive_sequence.checked_add(1).ok_or_else(|| {
                rejected("decrypt", "sequence_exhausted");
                CryptoError::SequenceExhausted
            })?;
        if packet.sequence_number != Some(expected) {
            rejected("decrypt", "sequence_mismatch");
            return Err(CryptoError::Sequence);
        }
        self.receive_sequence = expected;
        packet.message.ok_or_else(|| {
            rejected("decrypt", "missing_payload");
            CryptoError::InvalidMessage
        })
    }
}

/// Records and maps an authentication rejection.
fn authentication() -> CryptoError {
    rejected("decrypt", "authentication");
    CryptoError::Authentication
}

/// Records and maps a malformed secure-message field.
fn invalid_message(reason: &'static str) -> CryptoError {
    rejected("decrypt", reason);
    CryptoError::InvalidMessage
}

fn validate_header(header: &securemessage::Header) -> Result<(), CryptoError> {
    if header.signature_scheme != securemessage::SigScheme::HmacSha256 as i32
        || header.encryption_scheme
            != securemessage::EncScheme::Aes256Cbc as i32
    {
        rejected("decrypt", "unsupported_algorithm");
        return Err(CryptoError::Unsupported);
    }
    let metadata_bytes =
        header.public_metadata.as_deref().ok_or_else(|| {
            rejected("decrypt", "missing_metadata");
            CryptoError::InvalidMessage
        })?;
    let metadata =
        securegcm::GcmMetadata::decode(metadata_bytes).map_err(|_| {
            rejected("decrypt", "invalid_metadata");
            CryptoError::InvalidMessage
        })?;
    if metadata.r#type != securegcm::Type::DeviceToDeviceMessage as i32
        || metadata.version != Some(1_i32)
    {
        rejected("decrypt", "unsupported_version");
        return Err(CryptoError::Unsupported);
    }
    Ok(())
}

fn rejected(operation: &'static str, reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "d2d",
        operation,
        outcome = "rejected",
        reason,
        "D2D frame rejected"
    );
}
