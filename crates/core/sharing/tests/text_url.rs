//! Public text and URL Sharing protocol contracts.

#![expect(
    clippy::absolute_paths,
    clippy::expect_used,
    clippy::tests_outside_test_module,
    clippy::unwrap_used,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

use base64 as _;
use prost as _;
use quickshare_connections::{Connection, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_sharing::{OfferKind, ProtocolError, SharingSession};
use quickshare_wire::sharing::connection_response_frame as response;
use rand_core as _;
use serde as _;
use std::{
    net::{TcpListener, TcpStream},
    thread,
};

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

#[test]
fn decodes_google_text_and_url_introductions() {
    let incoming_text = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/text.bin"
    )))
    .expect("Google incoming text introduction");
    assert_eq!(incoming_text.kind(), OfferKind::Text);
    assert_eq!(incoming_text.name(), "fixture text");
    assert_eq!(incoming_text.size_bytes(), 12);
    assert_eq!(incoming_text.payload_id(), 202);

    let incoming_url = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/url.bin"
    )))
    .expect("Google incoming URL introduction");
    assert_eq!(incoming_url.kind(), OfferKind::Url);
    assert_eq!(incoming_url.name(), "https://x.y");
    assert_eq!(incoming_url.size_bytes(), 11);
    assert_eq!(incoming_url.payload_id(), 203);

    let outgoing_text = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "outgoing/introductions/text.bin"
    )))
    .expect("Google outgoing text introduction");
    assert_eq!(outgoing_text.kind(), OfferKind::Text);
    assert_eq!(outgoing_text.name(), "fixture text");
    assert_eq!(outgoing_text.size_bytes(), 12);
    assert_eq!(outgoing_text.payload_id(), 400);

    let outgoing_url = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "outgoing/introductions/url.bin"
    )))
    .expect("Google outgoing URL introduction");
    assert_eq!(outgoing_url.kind(), OfferKind::Url);
    assert_eq!(outgoing_url.name(), "https://x.y");
    assert_eq!(outgoing_url.size_bytes(), 11);
    assert_eq!(outgoing_url.payload_id(), 400);
}

#[test]
fn accepts_google_apk_introduction_as_a_file() {
    let offer = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/apk.bin"
    )))
    .expect("Google APK introduction");
    assert_eq!(offer.kind(), OfferKind::AndroidApp);
    assert!(offer.kind().persists_as_file());
    assert_eq!(offer.name(), "FixtureApp.apk");
    assert_eq!(offer.size_bytes(), 16);
    assert_eq!(offer.payload_id(), 204);
    assert_eq!(offer.package_name(), Some("dev.fixture.app"));
}

#[test]
fn loopback_sends_plain_text_after_consent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.kind(), OfferKind::Text);
        assert_eq!(offer.name(), "hello from omarchy");
        session.accept_incoming_offer().expect("accept offer");
        assert_eq!(
            session
                .receive_incoming_text(&offer, |_| {}, || false)
                .expect("receive text"),
            "hello from omarchy"
        );
    });

    let stream = TcpStream::connect(address).unwrap();
    let connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    session
        .send_outgoing_text("hello from omarchy", || {}, |_| {}, || false)
        .expect("send text after accept");
    receiver.join().expect("receiver completes");
}

#[test]
fn loopback_sends_url_after_consent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.kind(), OfferKind::Url);
        assert_eq!(offer.name(), "https://omarchy.local");
        session.accept_incoming_offer().expect("accept offer");
        assert_eq!(
            session
                .receive_incoming_url(&offer, |_| {}, || false)
                .expect("receive url"),
            "https://omarchy.local"
        );
    });

    let stream = TcpStream::connect(address).unwrap();
    let connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    session
        .send_outgoing_url("https://omarchy.local", || {}, |_| {}, || false)
        .expect("send url after accept");
    receiver.join().expect("receiver completes");
}

#[test]
fn text_rejection_reaches_the_outbound_sender() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.reject_incoming_offer().expect("reject offer");
    });

    let stream = TcpStream::connect(address).unwrap();
    let connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    let error = session
        .send_outgoing_text("rejected text", || {}, |_| {}, || false)
        .expect_err("peer rejection");
    assert!(matches!(error, ProtocolError::Rejected));
    receiver.join().expect("receiver completes");
}

#[test]
fn keepalive_before_text_introduction_is_ignored() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.kind(), OfferKind::Text);
        session.accept_incoming_offer().expect("accept offer");
        assert_eq!(
            session
                .receive_incoming_text(&offer, |_| {}, || false)
                .expect("receive text"),
            "ping"
        );
    });

    let stream = TcpStream::connect(address).unwrap();
    let connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    session.send_keepalive(1).expect("send keepalive");
    session
        .send_outgoing_text("ping", || {}, |_| {}, || false)
        .expect("send text after keepalive");
    receiver.join().expect("receiver completes");
}

#[test]
fn timed_out_and_unsupported_responses_are_distinct() {
    assert_eq!(
        SharingSession::decode_response(include_bytes!(concat!(
            "../../../../tests/fixtures/sharing/google-v1/",
            "incoming/responses/timed-out.bin"
        )))
        .expect("Google timed-out response"),
        quickshare_wire::sharing::connection_response_frame::Status::TimedOut
    );
    assert_eq!(
        SharingSession::decode_response(include_bytes!(concat!(
            "../../../../tests/fixtures/sharing/google-v1/",
            "incoming/responses/unsupported.bin"
        )))
        .expect("Google unsupported response"),
        response::Status::UnsupportedAttachmentType
    );

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.timeout_incoming_offer().expect("timeout offer");
    });

    let stream = TcpStream::connect(address).unwrap();
    let connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    let error = session
        .send_outgoing_text("late", || {}, |_| {}, || false)
        .expect_err("peer timeout");
    assert!(matches!(error, ProtocolError::TimedOut));
    receiver.join().expect("receiver completes");
}
