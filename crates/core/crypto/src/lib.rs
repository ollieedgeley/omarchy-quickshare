//! Byte-compatible UKEY2 and D2D secure-channel primitives.

#![forbid(unsafe_code)]
#![expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Handshake methods follow UKEY2 order, not alphabetical order"
)]
#![expect(
    clippy::as_conversions,
    reason = "prost uses required i32 wire values for protocol enum fields"
)]
#![expect(
    clippy::expect_used,
    reason = "Fixed primitive lengths make these invariant checks infallible"
)]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "Directional keys are visible only to sibling protocol code"
)]
#![expect(
    clippy::map_err_ignore,
    reason = "Protocol errors do not expose cryptographic library internals"
)]
#![expect(
    clippy::missing_inline_in_public_items,
    reason = "Public state-machine methods avoid cross-crate inlining"
)]
#![cfg_attr(
    not(test),
    expect(
        clippy::missing_docs_in_private_items,
        reason = "Private state-machine details follow public transitions"
    )
)]
#![expect(
    clippy::pub_use,
    reason = "The architecture requires a stable crate-root interface"
)]
#![expect(
    clippy::pub_with_shorthand,
    reason = "rustfmt scoped visibility conflicts with the restriction lint"
)]
#![expect(
    clippy::single_call_fn,
    reason = "Named crypto stages remain auditable at their call sites"
)]
#![expect(
    clippy::shadow_reuse,
    reason = "Decoding keeps wire names while narrowing validated values"
)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream client use"
    )
)]

use aes as _;
use cbc as _;
use core::fmt;
use hkdf as _;
use hmac as _;
use p256 as _;
use p256::{PublicKey, SecretKey, ecdh::diffie_hellman};
use prost as _;
use prost::Message as _;
use quickshare_wire as _;
use quickshare_wire::ukey2;
use rand_core as _;
use rand_core::{CryptoRng, RngCore};
use sha2 as _;
use sha2::{Digest as _, Sha256, Sha512};

#[macro_use]
mod diagnostics;
mod primitives;
mod secure_channel;
mod types;
use primitives::{d2d_key, derive, parse_public_key, public_key};
pub use secure_channel::SecureChannel;
pub use types::{CryptoError, HandshakeError, Role};

const AUTH_SALT: &[u8] = b"UKEY2 v1 auth\0";
const NEXT_PROTOCOL: &str = "AES_256_CBC-HMAC_SHA256";
const UKEY2_VERSION: i32 = 1;
const KEY_LENGTH: usize = 32;
const IV_LENGTH: usize = 16;
const NEXT_SALT: &[u8] = b"UKEY2 v1 next";

/// Outputs derived by one mutually completed UKEY2 exchange.
pub struct CompletedHandshake {
    /// Raw token used for human or headless peer verification.
    authentication_token: [u8; KEY_LENGTH],
    /// Directional encrypted channel for the next protocol.
    channel: SecureChannel,
}

impl fmt::Debug for CompletedHandshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompletedHandshake").finish_non_exhaustive()
    }
}

impl CompletedHandshake {
    /// Returns the raw shared peer-verification token.
    #[must_use]
    pub const fn authentication_token(&self) -> &[u8; 32] {
        &self.authentication_token
    }

    /// Consumes the handshake output and returns its directional channel.
    #[must_use]
    pub const fn into_channel(self) -> SecureChannel {
        self.channel
    }
}

/// A three-message P-256/SHA-512 UKEY2 exchange.
pub struct Handshake {
    role: Role,
    random: [u8; KEY_LENGTH],
    secret: SecretKey,
    state: State,
    client_init: Option<Vec<u8>>,
    server_init: Option<Vec<u8>>,
    commitment: Option<Vec<u8>>,
    peer_key: Option<PublicKey>,
}

impl fmt::Debug for Handshake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Handshake")
            .field("role", &self.role)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Start,
    WaitForPeer,
    SendFinish,
    Complete,
}

impl Handshake {
    /// Starts a deterministic initiator exchange.
    #[must_use]
    pub fn initiator(
        random: [u8; KEY_LENGTH],
        secret: [u8; KEY_LENGTH],
    ) -> Self {
        Self::new(Role::Initiator, random, secret)
    }

    /// Starts a deterministic responder exchange.
    #[must_use]
    pub fn responder(
        random: [u8; KEY_LENGTH],
        secret: [u8; KEY_LENGTH],
    ) -> Self {
        Self::new(Role::Responder, random, secret)
    }

    /// Starts an initiator exchange with caller-owned secure randomness.
    #[must_use]
    pub fn initiator_with_rng<R>(rng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        Self::with_rng(Role::Initiator, rng)
    }

    /// Starts a responder exchange with caller-owned secure randomness.
    #[must_use]
    pub fn responder_with_rng<R>(rng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        Self::with_rng(Role::Responder, rng)
    }

    fn new(
        role: Role,
        random: [u8; KEY_LENGTH],
        secret: [u8; KEY_LENGTH],
    ) -> Self {
        let secret =
            SecretKey::from_slice(&secret).expect("non-zero P-256 test secret");
        Self {
            role,
            random,
            secret,
            state: State::Start,
            client_init: None,
            server_init: None,
            commitment: None,
            peer_key: None,
        }
    }

    fn with_rng<R>(role: Role, rng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        let mut random = [0; KEY_LENGTH];
        rng.fill_bytes(&mut random);
        let secret = SecretKey::random(rng).to_bytes().into();
        Self::new(role, random, secret)
    }

    /// Returns this exchange's local role.
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// Produces the next UKEY2 message for the peer.
    ///
    /// # Errors
    ///
    /// Returns an error when called outside the next required handshake state.
    pub fn next_message(&mut self) -> Result<Vec<u8>, HandshakeError> {
        let role = self.role;
        let (event_type, result) = match (self.role, self.state) {
            (Role::Initiator, State::Start) => {
                ("client_init", self.client_init())
            }
            (Role::Responder, State::WaitForPeer) => {
                ("server_init", self.server_init())
            }
            (Role::Initiator, State::SendFinish) => {
                ("client_finish", self.client_finish())
            }
            _ => ("unexpected", Err(HandshakeError::InvalidState)),
        };
        ukey2_diagnostic!("prepare", role, event_type, &result);
        result
    }

    /// Accepts the next UKEY2 message from the peer.
    ///
    /// # Errors
    ///
    /// Returns an error for a malformed or invalid peer message in this state.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<(), HandshakeError> {
        let role = self.role;
        let (event_type, result) = match (self.role, self.state) {
            (Role::Responder, State::Start) => {
                ("client_init", self.receive_client_init(bytes))
            }
            (Role::Initiator, State::WaitForPeer) => {
                ("server_init", self.receive_server_init(bytes))
            }
            (Role::Responder, State::WaitForPeer) => {
                ("client_finish", self.receive_client_finish(bytes))
            }
            _ => ("unexpected", Err(HandshakeError::InvalidState)),
        };
        ukey2_diagnostic!("receive", role, event_type, &result);
        result
    }

    /// Derives the peer-verification token and directional D2D channel.
    ///
    /// # Errors
    ///
    /// Returns an error unless the full UKEY2 exchange completed successfully.
    pub fn complete(self) -> Result<CompletedHandshake, HandshakeError> {
        let role = self.role;
        let result = (|| {
            if self.state != State::Complete {
                return Err(HandshakeError::InvalidState);
            }
            let client_init =
                self.client_init.ok_or(HandshakeError::InvalidState)?;
            let server_init =
                self.server_init.ok_or(HandshakeError::InvalidState)?;
            let peer_key = self.peer_key.ok_or(HandshakeError::InvalidState)?;
            let shared = diffie_hellman(
                self.secret.to_nonzero_scalar(),
                peer_key.as_affine(),
            );
            let shared = Sha256::digest(shared.raw_secret_bytes());
            let transcript = [client_init, server_init].concat();
            let authentication_token =
                derive::<KEY_LENGTH>(&shared, AUTH_SALT, &transcript)
                    .map_err(|_| HandshakeError::KeyAgreement)?;
            let master = derive::<KEY_LENGTH>(&shared, NEXT_SALT, &transcript)
                .map_err(|_| HandshakeError::KeyAgreement)?;
            let client = d2d_key(&master, b"client")
                .map_err(|_| HandshakeError::KeyAgreement)?;
            let server = d2d_key(&master, b"server")
                .map_err(|_| HandshakeError::KeyAgreement)?;
            let (send, receive) = match self.role {
                Role::Initiator => (client, server),
                Role::Responder => (server, client),
            };
            Ok(CompletedHandshake {
                authentication_token,
                channel: SecureChannel::new(send, receive),
            })
        })();
        ukey2_diagnostic!("complete", role, "handshake", &result);
        result
    }

    fn client_init(&mut self) -> Result<Vec<u8>, HandshakeError> {
        let finish = wrap(
            ukey2::ukey2_message::Type::ClientFinish,
            ukey2::Ukey2ClientFinished {
                public_key: Some(public_key(&self.secret)?.encode_to_vec()),
            }
            .encode_to_vec(),
        );
        let commitment = Sha512::digest(&finish).to_vec();
        let body = ukey2::Ukey2ClientInit {
            version: Some(UKEY2_VERSION),
            random: Some(self.random.to_vec()),
            cipher_commitments: vec![
                ukey2::ukey2_client_init::CipherCommitment {
                    handshake_cipher: Some(
                        ukey2::Ukey2HandshakeCipher::P256Sha512 as i32,
                    ),
                    commitment: Some(commitment),
                },
            ],
            next_protocol: Some(NEXT_PROTOCOL.to_owned()),
        };
        let message =
            wrap(ukey2::ukey2_message::Type::ClientInit, body.encode_to_vec());
        self.client_init = Some(message.clone());
        self.state = State::WaitForPeer;
        Ok(message)
    }

    fn receive_client_init(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), HandshakeError> {
        let body = unwrap(bytes, ukey2::ukey2_message::Type::ClientInit)?;
        let init = ukey2::Ukey2ClientInit::decode(body.as_slice())
            .map_err(|_| HandshakeError::InvalidMessage)?;
        if init.version != Some(UKEY2_VERSION)
            || init
                .random
                .as_deref()
                .is_none_or(|value| value.len() != KEY_LENGTH)
        {
            return Err(HandshakeError::InvalidField);
        }
        if init.next_protocol.as_deref() != Some(NEXT_PROTOCOL) {
            return Err(HandshakeError::Unsupported);
        }
        let commitment = init
            .cipher_commitments
            .into_iter()
            .find_map(|item| {
                (item.handshake_cipher
                    == Some(ukey2::Ukey2HandshakeCipher::P256Sha512 as i32))
                .then_some(item.commitment)
                .flatten()
            })
            .filter(|value| !value.is_empty())
            .ok_or(HandshakeError::Unsupported)?;
        self.client_init = Some(bytes.to_vec());
        self.commitment = Some(commitment);
        self.state = State::WaitForPeer;
        Ok(())
    }

    fn server_init(&mut self) -> Result<Vec<u8>, HandshakeError> {
        let body = ukey2::Ukey2ServerInit {
            version: Some(UKEY2_VERSION),
            random: Some(self.random.to_vec()),
            handshake_cipher: Some(
                ukey2::Ukey2HandshakeCipher::P256Sha512 as i32,
            ),
            public_key: Some(public_key(&self.secret)?.encode_to_vec()),
        };
        let message =
            wrap(ukey2::ukey2_message::Type::ServerInit, body.encode_to_vec());
        self.server_init = Some(message.clone());
        Ok(message)
    }

    fn receive_server_init(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), HandshakeError> {
        let body = unwrap(bytes, ukey2::ukey2_message::Type::ServerInit)?;
        let init = ukey2::Ukey2ServerInit::decode(body.as_slice())
            .map_err(|_| HandshakeError::InvalidMessage)?;
        if init.version != Some(UKEY2_VERSION)
            || init
                .random
                .as_deref()
                .is_none_or(|value| value.len() != KEY_LENGTH)
        {
            return Err(HandshakeError::InvalidField);
        }
        if init.handshake_cipher
            != Some(ukey2::Ukey2HandshakeCipher::P256Sha512 as i32)
        {
            return Err(HandshakeError::Unsupported);
        }
        self.peer_key = Some(parse_public_key(
            init.public_key
                .as_deref()
                .ok_or(HandshakeError::InvalidField)?,
        )?);
        self.server_init = Some(bytes.to_vec());
        self.state = State::SendFinish;
        Ok(())
    }

    fn client_finish(&mut self) -> Result<Vec<u8>, HandshakeError> {
        let message = wrap(
            ukey2::ukey2_message::Type::ClientFinish,
            ukey2::Ukey2ClientFinished {
                public_key: Some(public_key(&self.secret)?.encode_to_vec()),
            }
            .encode_to_vec(),
        );
        self.state = State::Complete;
        Ok(message)
    }

    fn receive_client_finish(
        &mut self,
        bytes: &[u8],
    ) -> Result<(), HandshakeError> {
        let commitment = self
            .commitment
            .as_deref()
            .ok_or(HandshakeError::InvalidState)?;
        if Sha512::digest(bytes).as_slice() != commitment {
            return Err(HandshakeError::Commitment);
        }
        let body = unwrap(bytes, ukey2::ukey2_message::Type::ClientFinish)?;
        let finish = ukey2::Ukey2ClientFinished::decode(body.as_slice())
            .map_err(|_| HandshakeError::InvalidMessage)?;
        self.peer_key = Some(parse_public_key(
            finish
                .public_key
                .as_deref()
                .ok_or(HandshakeError::InvalidField)?,
        )?);
        self.state = State::Complete;
        Ok(())
    }
}

fn wrap(kind: ukey2::ukey2_message::Type, data: Vec<u8>) -> Vec<u8> {
    ukey2::Ukey2Message {
        message_type: Some(kind as i32),
        message_data: Some(data),
    }
    .encode_to_vec()
}
fn unwrap(
    bytes: &[u8],
    expected: ukey2::ukey2_message::Type,
) -> Result<Vec<u8>, HandshakeError> {
    let message = ukey2::Ukey2Message::decode(bytes)
        .map_err(|_| HandshakeError::InvalidMessage)?;
    if message.message_type != Some(expected as i32) {
        return Err(HandshakeError::InvalidMessage);
    }
    message.message_data.ok_or(HandshakeError::InvalidField)
}
