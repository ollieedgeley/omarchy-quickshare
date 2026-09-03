/// The local UKEY2 role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Role {
    /// Creates `CLIENT_INIT` and `CLIENT_FINISH`.
    Initiator,
    /// Creates `SERVER_INIT`.
    Responder,
}

/// A UKEY2 protocol failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum HandshakeError {
    /// A frame did not contain the expected protobuf message.
    InvalidMessage,
    /// A required UKEY2 field was absent or malformed.
    InvalidField,
    /// A message arrived in an invalid handshake state.
    InvalidState,
    /// The peer selected an unsupported protocol or cipher.
    Unsupported,
    /// `CLIENT_FINISH` did not match the advertised commitment.
    Commitment,
    /// The peer's P-256 public key was invalid.
    PublicKey,
    /// Key agreement or key derivation failed.
    KeyAgreement,
}

/// A D2D secure-message failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CryptoError {
    /// A protobuf message was malformed.
    InvalidMessage,
    /// The encrypted frame used unsupported metadata or algorithms.
    Unsupported,
    /// The frame HMAC did not verify.
    Authentication,
    /// AES-CBC decryption or PKCS#7 unpadding failed.
    Decryption,
    /// The received sequence number was not the next expected value.
    Sequence,
    /// A sequence number cannot be incremented.
    SequenceExhausted,
}
