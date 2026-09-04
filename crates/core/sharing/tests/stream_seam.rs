//! Public Sharing contracts over a generic byte-stream seam.

#![expect(
    clippy::absolute_paths,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::missing_trait_methods,
    clippy::panic_in_result_fn,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

use base64 as _;
use core::cell::Cell;
use prost as _;
use quickshare_connections::{Connection, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_sharing::{
    OfferKind, PairingStatus, ProtocolError, SharingSession,
};
use quickshare_wire as _;
use rand_core as _;
use serde as _;
use std::{
    io::{Cursor, Read},
    os::unix::net::UnixStream,
    thread,
};

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];
const MULTI_FRAME_FILE_SIZE: usize = 0x0010_0001;

struct AcceptedReader<'accepted> {
    accepted: &'accepted Cell<bool>,
    cursor: Cursor<Vec<u8>>,
}

impl Read for AcceptedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        assert!(self.accepted.get());
        self.cursor.read(buf)
    }
}

#[test]
fn unix_pair_connect_io_establishes_account_free_pairing() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let responder = thread::spawn(move || {
        let mut session =
            SharingSession::accept_io(responder_stream, "remote", "Remote")
                .expect("establish peer session");
        assert_eq!(
            session.exchange_account_free_pairing().expect("pair"),
            PairingStatus::Unable
        );
    });

    let mut session =
        SharingSession::connect_io(initiator_stream, "local", "Omarchy")
            .expect("establish local session");
    assert_eq!(
        session.exchange_account_free_pairing().expect("pair"),
        PairingStatus::Unable
    );
    responder.join().expect("responder completes");
}

#[test]
fn unix_pair_sends_file_after_consent() {
    let bytes = vec![0xA5; MULTI_FRAME_FILE_SIZE];
    let expected = bytes.clone();
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let receiver = thread::spawn(move || {
        let connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        assert_eq!(
            session.exchange_account_free_pairing().expect("pair"),
            PairingStatus::Unable
        );
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.name(), "note.txt");
        session.accept_incoming_offer().expect("accept offer");
        let mut received = Vec::new();
        session
            .receive_incoming_file(&offer, &mut received, |_| {}, || false)
            .expect("receive file");
        assert_eq!(received, expected);
    });

    let connection = Connection::connect_io(
        initiator_stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    assert_eq!(
        session.exchange_account_free_pairing().expect("pair"),
        PairingStatus::Unable
    );
    let accepted = Cell::new(false);
    let mut reader = AcceptedReader {
        accepted: &accepted,
        cursor: Cursor::new(bytes),
    };
    session
        .send_outgoing_file(
            "note.txt",
            u64::try_from(MULTI_FRAME_FILE_SIZE).expect("file size"),
            &mut reader,
            || accepted.set(true),
            |_| {},
            || false,
        )
        .expect("send file after accept");
    assert!(accepted.get());
    receiver.join().expect("receiver completes");
}

#[test]
fn unix_pair_sends_plain_text_after_consent() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let receiver = thread::spawn(move || {
        let connection = Connection::accept_io(
            responder_stream,
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

    let connection = Connection::connect_io(
        initiator_stream,
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
fn unix_pair_sends_url_after_consent() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let receiver = thread::spawn(move || {
        let connection = Connection::accept_io(
            responder_stream,
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

    let connection = Connection::connect_io(
        initiator_stream,
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
fn unix_pair_text_rejection_reaches_the_outbound_sender() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let receiver = thread::spawn(move || {
        let mut session =
            SharingSession::accept_io(responder_stream, "remote", "Remote")
                .expect("establish peer session");
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.reject_incoming_offer().expect("reject offer");
    });

    let mut session =
        SharingSession::connect_io(initiator_stream, "local", "Omarchy")
            .expect("establish local session");
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    let error = session
        .send_outgoing_text("rejected text", || {}, |_| {}, || false)
        .expect_err("peer rejection");
    assert!(matches!(error, ProtocolError::Rejected));
    receiver.join().expect("receiver completes");
}
