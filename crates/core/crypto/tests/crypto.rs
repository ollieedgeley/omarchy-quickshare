//! Public UKEY2 and D2D crypto contract tests.

#![expect(
    clippy::expect_used,
    clippy::shadow_reuse,
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    unused_results,
    reason = "The direct harness asserts invariant protocol steps clearly"
)]

use aes as _;
use cbc as _;
use hkdf as _;
use hmac as _;
use p256 as _;
use prost as _;
use quickshare_crypto::{
    CompletedHandshake, Handshake, HandshakeError, Role, SecureChannel,
};
use quickshare_wire as _;
use rand_core as _;
use sha2 as _;
use tracing as _;
const AUTHENTICATION_TOKEN: [u8; 32] = [
    0x2a, 0xf9, 0x8f, 0xce, 0xeb, 0x2c, 0x90, 0xaa, 0xcd, 0xfd, 0x93, 0x78,
    0x0c, 0x85, 0xfb, 0x6c, 0xa0, 0x6d, 0xcc, 0x51, 0xb2, 0xa0, 0x07, 0xd9,
    0xc1, 0x79, 0xa5, 0x0d, 0x11, 0x36, 0x00, 0x5b,
];

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

fn completed_exchange() -> (CompletedHandshake, CompletedHandshake) {
    let mut initiator =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    let mut responder =
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);
    let first = initiator
        .next_message()
        .expect("initiator starts the exchange");
    responder
        .receive(&first)
        .expect("responder accepts client init");
    let second = responder.next_message().expect("responder replies");
    initiator
        .receive(&second)
        .expect("initiator accepts server init");
    let third = initiator.next_message().expect("initiator finishes");
    responder
        .receive(&third)
        .expect("responder verifies commitment");
    let initiator =
        initiator.complete().expect("initiator completes handshake");
    let responder =
        responder.complete().expect("responder completes handshake");
    (initiator, responder)
}

fn exchange() -> (SecureChannel, SecureChannel) {
    let (initiator, responder) = completed_exchange();
    (initiator.into_channel(), responder.into_channel())
}

#[test]
fn completed_handshake_exposes_a_shared_authentication_token() {
    let (initiator, responder) = completed_exchange();

    assert_eq!(
        initiator.authentication_token(),
        responder.authentication_token()
    );
    assert_eq!(initiator.authentication_token(), &AUTHENTICATION_TOKEN);
}

#[test]
fn initiator_and_responder_exchange_byte_compatible_ukey2_messages() {
    let (mut initiator, mut responder) = exchange();
    let message = initiator.encrypt(b"hello", [5; 16]).expect("encrypt");

    assert_eq!(responder.decrypt(&message).expect("decrypt"), b"hello");
    assert_eq!(initiator.session_unique(), responder.session_unique());
}

#[test]
fn responder_rejects_a_client_finish_with_the_wrong_commitment() {
    let mut initiator =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    let mut responder =
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);
    let first = initiator.next_message().expect("initiator starts");
    responder
        .receive(&first)
        .expect("responder accepts client init");
    let second = responder.next_message().expect("responder replies");
    initiator
        .receive(&second)
        .expect("initiator accepts server init");
    let mut third = initiator.next_message().expect("initiator finishes");
    let last = third.last_mut().expect("client finish has bytes");
    *last ^= 1;

    assert_eq!(responder.receive(&third), Err(HandshakeError::Commitment));
}

#[test]
fn channel_rejects_wrong_hmac_without_advancing_its_sequence() {
    let (mut initiator, mut responder) = exchange();
    let mut message = initiator.encrypt(b"one", [6; 16]).expect("encrypt");
    let valid = message.clone();
    let last = message.last_mut().expect("secure message has HMAC");
    *last ^= 1;

    responder.decrypt(&message).unwrap_err();
    assert_eq!(
        responder.decrypt(&valid).expect("sequence stays at one"),
        b"one"
    );
}

#[test]
fn channel_rejects_replayed_and_out_of_order_messages() {
    let (mut initiator, mut responder) = exchange();
    let first = initiator.encrypt(b"one", [8; 16]).expect("encrypt");
    let second = initiator.encrypt(b"two", [9; 16]).expect("encrypt");

    responder.decrypt(&second).unwrap_err();
    assert_eq!(responder.decrypt(&first).expect("first frame"), b"one");
    responder.decrypt(&first).unwrap_err();
}

#[test]
fn handshake_accepts_a_significant_leading_zero_coordinate() {
    let mut responder_secret = [0; 32];
    responder_secret[31] = 43;
    let mut initiator =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    let mut responder =
        Handshake::responder(RESPONDER_RANDOM, responder_secret);
    let first = initiator.next_message().expect("initiator starts");
    responder
        .receive(&first)
        .expect("responder accepts client init");
    let second = responder.next_message().expect("responder replies");

    initiator
        .receive(&second)
        .expect("initiator accepts a 32-byte coordinate beginning with zero");
}

#[test]
fn roles_are_explicit_at_the_public_handshake_seam() {
    assert_eq!(
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET).role(),
        Role::Initiator
    );
    assert_eq!(
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET).role(),
        Role::Responder
    );
}
