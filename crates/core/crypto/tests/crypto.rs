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
use quickshare_crypto::{Handshake, HandshakeError, Role, SecureChannel};
use quickshare_wire as _;
use rand_core as _;
use sha2 as _;

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

fn exchange() -> (SecureChannel, SecureChannel) {
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
    let initiator = initiator
        .into_channel()
        .expect("initiator completes handshake");
    let responder = responder
        .into_channel()
        .expect("responder completes handshake");
    (initiator, responder)
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
