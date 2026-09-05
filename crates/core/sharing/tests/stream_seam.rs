//! Public Sharing contracts over a generic byte-stream seam.

#![expect(
    clippy::absolute_paths,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::missing_trait_methods,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    clippy::wildcard_enum_match_arm,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

#[expect(
    unreachable_pub,
    reason = "Shared constants are exported only through private test modules"
)]
#[path = "common/pairing.rs"]
mod pairing;
#[expect(
    clippy::single_call_fn,
    unreachable_pub,
    reason = "Shared fixtures are reused across integration-test crates"
)]
#[path = "common/stream.rs"]
mod stream;

use base64 as _;
use core::{cell::Cell, time::Duration};
use pairing::{
    INITIATOR_RANDOM, INITIATOR_SECRET, RESPONDER_RANDOM, RESPONDER_SECRET,
};
use prost as _;
use quickshare_connections::{Connection, ConnectionOptions, Event, Medium};
use quickshare_crypto::Handshake;
use quickshare_sharing::{
    OfferKind, PairingStatus, PairingStep, ProtocolError, SharingSession,
};
use quickshare_wire::sharing::connection_response_frame;
use rand_core as _;
use serde as _;
use std::{
    io::{Cursor, Read},
    os::unix::net::UnixStream,
    sync::mpsc,
    thread,
};
use stream::{account_free_encryption, connect_paired_initiator};
use tracing as _;
use tracing_subscriber as _;

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
fn pairing_reports_receive_encryption_when_peer_closes_after_local_encryption()
{
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        assert!(matches!(
            connection.receive().expect("receive local encryption"),
            Event::Bytes { .. }
        ));
    });

    let connection = Connection::connect_io(
        initiator_stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let error = SharingSession::new(connection)
        .exchange_account_free_pairing()
        .expect_err("peer closed before encryption response");

    assert_eq!(error.step(), PairingStep::ReceiveEncryption);
    assert!(matches!(error.source_error(), ProtocolError::Disconnected));
    responder.join().expect("responder completes");
}

#[test]
fn paired_key_payload_ids_are_not_reused_by_introduction() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let encryption_id =
            match connection.receive().expect("receive local encryption") {
                Event::Bytes { id, .. } => id,
                other => panic!("expected encryption bytes, got {other:?}"),
            };
        connection
            .send_sharing_frame(1, &account_free_encryption())
            .expect("send account-free encryption");
        let result_id = match connection
            .receive()
            .expect("receive local paired-key result")
        {
            Event::Bytes { id, .. } => id,
            other => panic!("expected result bytes, got {other:?}"),
        };
        connection
            .send_sharing_frame(2, &SharingSession::account_free_result())
            .expect("send unable");
        let introduction_id =
            match connection.receive().expect("receive local introduction") {
                Event::Bytes { id, .. } => id,
                other => panic!("expected introduction bytes, got {other:?}"),
            };
        (encryption_id, result_id, introduction_id)
    });

    let mut session = connect_paired_initiator(initiator_stream);
    let _closed = session
        .send_outgoing_text("hello from omarchy", || {}, |_| {}, || false)
        .expect_err("peer closed after introduction");
    let (encryption_id, result_id, introduction_id) =
        responder.join().expect("responder completes");
    assert_ne!(encryption_id, result_id);
    assert_ne!(encryption_id, introduction_id);
    assert_ne!(result_id, introduction_id);
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
fn outbound_url_completes_while_receiver_connection_remains_open() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let (received_sender, received) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel();
    let peer = thread::spawn(move || {
        let mut session =
            SharingSession::accept_io(responder_stream, "remote", "Remote")
                .expect("establish peer session");
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let offer = session.receive_incoming_offer().expect("receive offer");
        session.accept_incoming_offer().expect("accept offer");
        let value = session
            .receive_incoming_url(&offer, |_| {}, || false)
            .expect("receive URL");
        received_sender.send(value).expect("report received URL");
        release_receiver.recv().expect("release receiver");
    });
    let (completed_sender, completed) = mpsc::channel();
    let sender = thread::spawn(move || {
        let mut session =
            SharingSession::connect_io(initiator_stream, "local", "Omarchy")
                .expect("establish local session");
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        let result = session.send_outgoing_url(
            "https://omarchy.local",
            || {},
            |_| {},
            || false,
        );
        completed_sender.send(result).expect("report sender result");
    });

    assert_eq!(
        received.recv_timeout(Duration::from_secs(1)).expect("URL"),
        "https://omarchy.local"
    );
    let result = completed.recv_timeout(Duration::from_secs(1));
    release.send(()).expect("release receiver");
    peer.join().expect("receiver completes");
    sender.join().expect("sender completes");
    assert!(matches!(result, Ok(Ok(()))), "{result:?}");
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

#[test]
fn pending_consent_survives_upgrade_failure_before_file_acceptance() {
    const FILE_BYTES: &[u8; 12] = b"hello world\n";
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    for stream in [&initiator_stream, &responder_stream] {
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("bound peer reads");
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .expect("bound peer writes");
    }
    let (ready_sender, ready) = mpsc::channel();
    let (polled_sender, polled) = mpsc::channel::<bool>();
    let peer = thread::spawn(move || {
        let mut connection = Connection::connect_io(
            initiator_stream,
            Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish encrypted peer");
        connection
            .send_bytes(
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../../tests/fixtures/sharing/google-v1/",
                    "incoming/introductions/file.bin"
                )),
            )
            .expect("send Google introduction");
        connection
            .fail_upgrade(Medium::WifiHotspot)
            .expect("report failed upgrade");
        ready_sender.send(()).expect("report ready control");
        if !polled
            .recv_timeout(Duration::from_secs(1))
            .expect("receiver polled upgrade failure")
        {
            return;
        }
        assert!(
            connection
                .poll_event()
                .expect("check pending consent")
                .is_none()
        );
        ready_sender
            .send(())
            .expect("confirm no implicit acceptance");
        let Event::Bytes { bytes, .. } =
            connection.receive().expect("receive local decision")
        else {
            panic!("expected Sharing consent response");
        };
        assert_eq!(
            SharingSession::decode_response(&bytes).expect("decode decision"),
            connection_response_frame::Status::Accept
        );
        connection
            .send_file_header(201, 12, Some(String::from("fixture-file.bin")))
            .expect("declare accepted payload");
        connection
            .send_file_chunk(201, 0, FILE_BYTES, false)
            .expect("send accepted bytes");
        connection
            .send_file_chunk(201, 12, &[], true)
            .expect("finish accepted payload");
    });
    let connection = Connection::accept_io(
        responder_stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish encrypted receiver");
    let mut session = SharingSession::new(connection);
    let offer = session.receive_incoming_offer().expect("receive offer");
    ready
        .recv_timeout(Duration::from_secs(1))
        .expect("upgrade failure is ready");
    let pending = session.poll_pending_consent_control();
    polled_sender
        .send(pending.is_ok())
        .expect("report completed poll");
    if let Err(error) = pending {
        peer.join().expect("release peer after failed poll");
        panic!("failed upgrade must preserve pending consent: {error}");
    }
    ready
        .recv_timeout(Duration::from_secs(1))
        .expect("peer observed no implicit acceptance");
    session
        .accept_incoming_offer()
        .expect("explicitly accept offer");
    let mut received = Vec::new();
    session
        .receive_incoming_file(&offer, &mut received, |_| {}, || false)
        .expect("receive accepted file on original connection");
    peer.join().expect("peer completes");
    assert_eq!(received, FILE_BYTES);
}
