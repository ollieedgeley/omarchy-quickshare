//! Public Sharing protocol contracts.

#![expect(
    clippy::expect_used,
    reason = "Contract assertions label unexpected fixture or loopback failures"
)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Cargo compiles this as an integration-test crate"
)]

use core::cell::Cell;

use base64 as _;
use prost::Message as _;
use quickshare_connections::{Connection, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_sharing::{
    EndpointInfo, MdnsInstance, PairingStatus, SharingSession,
};
use quickshare_wire::sharing::{
    FileMetadata, Frame, IntroductionFrame, PairedKeyResultFrame, V1Frame,
    connection_response_frame, file_metadata, v1_frame,
};
use rand_core as _;
use serde as _;
use std::{
    io::{self, Cursor, Read},
    net::{TcpListener, TcpStream},
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

#[expect(
    clippy::missing_trait_methods,
    reason = "Default Read helpers delegate through the asserted read method"
)]
impl Read for AcceptedReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.accepted.get() {
            return Err(io::Error::other(
                "peer acceptance must be reported before payload reads",
            ));
        }
        self.cursor.read(buf)
    }
}

#[test]
fn endpoint_info_and_lan_instance_round_trip_google_layout() {
    let endpoint_info = EndpointInfo::new(
        0,
        1,
        [0xAA, 0xBB],
        [0x11; 14],
        Some("Omarchy"),
        Some(1),
        vec![7, 8],
    )
    .expect("valid endpoint info");
    assert_eq!(
        EndpointInfo::decode(&endpoint_info.encode()).expect("decode"),
        endpoint_info
    );

    let instance = MdnsInstance::new(*b"ABCD");
    assert_eq!(
        instance.encode().as_slice(),
        &[0x23, b'A', b'B', b'C', b'D', 0xFC, 0x9F, 0x5E, 0, 0]
    );
    assert_eq!(instance.label(), "I0FCQ0T8n14AAA");
    assert_eq!(
        MdnsInstance::decode_label("I0FCQ0T8n14AAA")
            .expect("decode instance label"),
        instance
    );
    assert_eq!(
        EndpointInfo::decode_property(&endpoint_info.property())
            .expect("decode endpoint property"),
        endpoint_info
    );
    assert_eq!(endpoint_info.device_name(), Some("Omarchy"));
    assert_eq!(
        MdnsInstance::decode(&instance.encode()).expect("decode instance"),
        instance
    );
    assert_eq!(MdnsInstance::service_type(), "_FC9F5ED42C8A._tcp.local.");
}

#[test]
fn decodes_google_file_introduction_and_accept_response() {
    let offer = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/file.bin"
    )))
    .expect("Google file introduction");
    assert_eq!(offer.name(), "fixture-file.bin");
    assert_eq!(offer.size_bytes(), 12);
    assert_eq!(offer.payload_id(), 201);

    assert_eq!(
        SharingSession::decode_response(include_bytes!(concat!(
            "../../../../tests/fixtures/sharing/google-v1/",
            "incoming/responses/accept.bin"
        )))
        .expect("Google accept response"),
        connection_response_frame::Status::Accept
    );
}

#[test]
fn decodes_google_outgoing_file_introduction() {
    let offer = SharingSession::decode_offer(include_bytes!(concat!(
        "../../../../tests/fixtures/sharing/google-v1/",
        "outgoing/introductions/file.bin"
    )))
    .expect("Google outgoing file introduction");
    assert_eq!(offer.name(), "quickshare-fixture.bin");
    assert_eq!(offer.size_bytes(), 12);
}

#[test]
fn decodes_google_file_introduction_with_negative_payload_identifier() {
    let frame = Frame {
        version: Some(1_i32),
        v1: Some(V1Frame {
            r#type: Some(i32::from(v1_frame::FrameType::Introduction)),
            introduction: Some(IntroductionFrame {
                file_metadata: vec![FileMetadata {
                    name: Some(String::from("signed-id.bin")),
                    r#type: Some(i32::from(file_metadata::Type::Document)),
                    payload_id: Some(-7),
                    size: Some(3),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }),
    };
    let offer = SharingSession::decode_offer(&frame.encode_to_vec())
        .expect("signed payload identifiers are opaque");
    assert_eq!(offer.payload_id(), -7);
}

#[test]
fn outbound_tcp_connect_establishes_encryption_with_a_loopback_peer() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let address = listener.local_addr().expect("listener address");
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept peer");
        let mut session = SharingSession::accept(stream, "remote", "Remote")
            .expect("establish peer session");
        assert_eq!(
            session.exchange_account_free_pairing().expect("pair"),
            PairingStatus::Unable
        );
    });

    let stream = TcpStream::connect(address).expect("connect peer");
    let mut session = SharingSession::connect(stream, "local", "Omarchy")
        .expect("establish local session");
    assert_eq!(
        session.exchange_account_free_pairing().expect("pair"),
        PairingStatus::Unable
    );
    responder.join().expect("responder completes");
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "One loopback flow keeps consent and payload order observable"
)]
fn account_free_pairing_chunks_one_file_across_connection_events() {
    let bytes = vec![0xA5; MULTI_FRAME_FILE_SIZE];
    let expected = bytes.clone();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let address = listener.local_addr().expect("listener address");
    let receiver = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept peer");
        let connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        assert_eq!(session.verification_code(), "9418");
        assert_eq!(
            session.exchange_account_free_pairing().expect("pair"),
            PairingStatus::Unable
        );
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.name(), "note.txt");
        session.accept_incoming_offer().expect("accept offer");
        let mut received = Vec::new();
        session
            .receive_incoming_file(&offer, &mut received)
            .expect("receive file");
        assert_eq!(received, expected);
    });

    let stream = TcpStream::connect(address).expect("connect peer");
    let connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    assert_eq!(session.verification_code(), "9418");
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
        )
        .expect("send file after accept");
    assert!(accepted.get());
    receiver.join().expect("receiver completes");
}

#[test]
fn account_free_pairing_writes_unable_result() {
    let result = Frame::decode(
        SharingSession::account_free_result()
            .encode_to_vec()
            .as_slice(),
    )
    .expect("result frame");
    let status = result
        .v1
        .and_then(|frame| frame.paired_key_result)
        .and_then(|frame: PairedKeyResultFrame| frame.status);
    assert_eq!(status, Some(3_i32));
}
