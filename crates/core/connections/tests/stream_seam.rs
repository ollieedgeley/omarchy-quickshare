//! Public Connections contracts over a generic byte-stream seam.

#![expect(
    clippy::expect_used,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

use prost::Message as _;
use quickshare_connections::{
    Connection, ConnectionOptions, Event, Medium, UpgradeCredentials,
    UpgradeEvent, UpgradeState,
};
use quickshare_crypto::Handshake;
use quickshare_wire::sharing::Frame;
use rand_core as _;
use std::{io::Write as _, os::unix::net::UnixStream, thread};
use tracing as _;

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

#[test]
fn unix_pair_encrypts_sharing_bytes_and_file_chunks() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder"),
        )
        .expect("establish responder encryption");
        assert_eq!(
            connection.receive().expect("receive sharing bytes"),
            Event::Bytes {
                id: 7,
                bytes: Frame::default().encode_to_vec(),
            }
        );
        assert_eq!(
            connection.receive().expect("receive file header"),
            Event::FileHeader {
                id: 9,
                total_size: 3,
                name: Some("note.txt".into()),
            }
        );
        assert_eq!(
            connection.receive().expect("receive file chunk"),
            Event::FileChunk {
                id: 9,
                offset: 0,
                bytes: b"abc".to_vec(),
                is_last: true,
            }
        );
        connection.disconnect().expect("send disconnection");
    });

    let mut connection = Connection::connect_io(
        initiator_stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator"),
    )
    .expect("establish initiator encryption");
    connection
        .send_sharing_frame(7, &Frame::default())
        .expect("send sharing frame as bytes payload");
    connection
        .send_file_header(9, 3, Some("note.txt".into()))
        .expect("send file header");
    connection
        .send_file_chunk(9, 0, b"abc", true)
        .expect("send file chunk");
    assert_eq!(
        connection.receive().expect("receive disconnect"),
        Event::Disconnected
    );
    responder.join().expect("responder completes");
}

#[test]
fn unix_pair_upgrade_failure_keeps_payload_on_original_medium() {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Bluetooth),
        )
        .expect("establish responder encryption");
        assert_eq!(
            connection.receive().expect("receive file header"),
            Event::FileHeader {
                id: 9,
                total_size: 3,
                name: Some("note.txt".into()),
            }
        );
        assert_eq!(
            connection.receive().expect("receive first file chunk"),
            Event::FileChunk {
                id: 9,
                offset: 0,
                bytes: b"a".to_vec(),
                is_last: false,
            }
        );
        assert_eq!(
            connection.receive().expect("receive upgrade offer"),
            Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium: Medium::WifiLan,
                    credentials: UpgradeCredentials::default(),
                },
            }
        );
        connection
            .complete_upgrade(Medium::WifiLan)
            .expect("accept offered medium");
        assert_eq!(
            connection.receive().expect("receive upgrade failure"),
            Event::Upgrade {
                event: UpgradeEvent::Failure {
                    medium: Medium::WifiHotspot,
                },
            }
        );
        assert_eq!(
            connection.upgrade_state(),
            UpgradeState::Failed {
                attempted: Medium::WifiHotspot,
                fallback: Medium::WifiLan,
            }
        );
        assert_eq!(connection.medium(), Medium::WifiLan);
        assert_eq!(
            connection.receive().expect("receive file chunk"),
            Event::FileChunk {
                id: 9,
                offset: 1,
                bytes: b"bc".to_vec(),
                is_last: true,
            }
        );
    });

    let mut connection = Connection::connect_io(
        initiator_stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Bluetooth),
    )
    .expect("establish initiator encryption");
    connection
        .send_file_header(9, 3, Some("note.txt".into()))
        .expect("declare file before upgrade");
    connection
        .send_file_chunk(9, 0, b"a", false)
        .expect("start file before upgrade");
    connection
        .propose_upgrade(Medium::WifiLan)
        .expect("offer LAN upgrade");
    connection
        .complete_upgrade(Medium::WifiLan)
        .expect("finish LAN upgrade");
    connection
        .fail_upgrade(Medium::WifiHotspot)
        .expect("report hotspot failure");
    assert_eq!(connection.medium(), Medium::WifiLan);
    connection
        .send_file_chunk(9, 1, b"bc", true)
        .expect("finish file on fallback medium");
    responder.join().expect("responder completes");
}

#[test]
fn unix_pair_complete_upgrade_io_continues_payload_on_new_stream() {
    let (old_initiator, old_responder) = UnixStream::pair().expect("old pair");
    let (new_initiator, new_responder) = UnixStream::pair().expect("new pair");
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            old_responder,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Bluetooth),
        )
        .expect("establish responder encryption");
        assert_eq!(
            connection.receive().expect("receive upgrade offer"),
            Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium: Medium::WifiLan,
                    credentials: UpgradeCredentials::default(),
                },
            }
        );
        connection
            .complete_upgrade_io(Medium::WifiLan, new_responder)
            .expect("carry session on upgraded stream");
        assert_eq!(connection.medium(), Medium::WifiLan);
        assert_eq!(
            connection.receive().expect("receive bytes after upgrade"),
            Event::Bytes {
                id: 11,
                bytes: b"after".to_vec(),
            }
        );
    });

    let mut connection = Connection::connect_io(
        old_initiator,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Bluetooth),
    )
    .expect("establish initiator encryption");
    connection
        .propose_upgrade(Medium::WifiLan)
        .expect("offer LAN upgrade");
    connection
        .complete_upgrade_io(Medium::WifiLan, new_initiator)
        .expect("continue session on upgraded stream");
    connection
        .send_bytes(11, b"after")
        .expect("send bytes after medium switch");
    responder.join().expect("responder completes");
}

#[test]
fn clean_eof_disconnects_but_truncated_frames_are_io_errors() {
    let connect = |tail: &[u8]| {
        let (receiver_stream, sender_stream) =
            UnixStream::pair().expect("unix pair");
        let mut raw_sender =
            sender_stream.try_clone().expect("clone sender stream");
        let copied_tail = tail.to_vec();
        let sender = thread::spawn(move || {
            let _connection = Connection::connect_io(
                sender_stream,
                Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
                ConnectionOptions::new("initiator", "initiator"),
            )
            .expect("establish sender encryption");
            raw_sender
                .write_all(&copied_tail)
                .expect("write raw frame tail");
            raw_sender.flush().expect("flush raw frame tail");
        });
        let connection = Connection::accept_io(
            receiver_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder"),
        )
        .expect("establish receiver encryption");
        (connection, sender)
    };

    let (mut clean, clean_sender) = connect(&[]);
    assert_eq!(
        clean.receive().expect("clean EOF on a frame boundary"),
        Event::Disconnected
    );
    clean_sender.join().expect("clean sender completes");

    for (tail, label) in [
        (&[0, 0][..], "partial frame prefix"),
        (&[0, 0, 0, 4, 0][..], "partial declared frame body"),
    ] {
        let (mut connection, sender) = connect(tail);
        assert!(
            matches!(
                connection.receive(),
                Err(quickshare_connections::Error::Io(_))
            ),
            "{label} must remain an I/O error"
        );
        sender.join().expect("truncating sender completes");
    }
}
