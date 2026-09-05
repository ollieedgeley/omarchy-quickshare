//! Public Sharing protocol contracts.

#![expect(
    clippy::expect_used,
    clippy::missing_assert_message,
    reason = "Contract assertions label unexpected fixture or loopback failures"
)]
#![expect(
    clippy::tests_outside_test_module,
    reason = "Cargo compiles this as an integration-test crate"
)]

mod common;

use core::cell::Cell;

use base64 as _;
use common::{
    accept_stream, bind_loopback, connect_session, connect_stream,
    decode_google_offer, google_v1, paired_loopback, spawn_peer,
};
use prost::Message as _;
use quickshare_sharing::{
    EndpointInfo, MdnsInstance, OfferKind, PairingStatus, ProtocolError,
    SharingSession,
};
use quickshare_wire::sharing::{
    AppMetadata, FileMetadata, Frame, IntroductionFrame, PairedKeyResultFrame,
    V1Frame, connection_response_frame, file_metadata, v1_frame,
};
use rand_core as _;
use serde as _;
use std::{
    io::{self, Cursor, Read},
    thread,
};
use tracing as _;
use tracing_subscriber as _;

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

fn introduction_frame(introduction: IntroductionFrame) -> Frame {
    Frame {
        version: Some(1_i32),
        v1: Some(V1Frame {
            r#type: Some(i32::from(v1_frame::FrameType::Introduction)),
            introduction: Some(introduction),
            ..Default::default()
        }),
    }
}

fn pair_unable(session: &mut SharingSession) {
    assert_eq!(
        session.exchange_account_free_pairing().expect("pair"),
        PairingStatus::Unable
    );
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
    let offer = decode_google_offer(
        google_v1!("incoming/introductions/file.bin"),
        "Google file introduction",
    );
    assert_eq!(offer.name(), "fixture-file.bin");
    assert_eq!(offer.size_bytes(), 12);
    assert_eq!(offer.payload_id(), 201);

    assert_eq!(
        SharingSession::decode_response(google_v1!(
            "incoming/responses/accept.bin"
        ))
        .expect("Google accept response"),
        connection_response_frame::Status::Accept
    );
}

#[test]
fn decodes_google_outgoing_file_introduction() {
    let offer = decode_google_offer(
        google_v1!("outgoing/introductions/file.bin"),
        "Google outgoing file introduction",
    );
    assert_eq!(offer.name(), "quickshare-fixture.bin");
    assert_eq!(offer.size_bytes(), 12);
}

#[test]
fn decodes_google_file_introduction_with_negative_payload_identifier() {
    let frame = introduction_frame(IntroductionFrame {
        file_metadata: vec![FileMetadata {
            name: Some(String::from("signed-id.bin")),
            r#type: Some(i32::from(file_metadata::Type::Document)),
            payload_id: Some(-7),
            size: Some(3),
            ..Default::default()
        }],
        ..Default::default()
    });
    let offer = SharingSession::decode_offer(&frame.encode_to_vec())
        .expect("signed payload identifiers are opaque");
    assert_eq!(offer.payload_id(), -7);
}

#[test]
fn decodes_aligned_split_apk_introduction() {
    let offer = SharingSession::decode_offer(
        &introduction_frame(IntroductionFrame {
            app_metadata: vec![AppMetadata {
                app_name: Some(String::from("Chat")),
                size: Some(24),
                payload_id: vec![11, 12],
                file_name: vec![
                    String::from("base.apk"),
                    String::from("config.apk"),
                ],
                file_size: vec![16, 8],
                package_name: Some(String::from("dev.chat")),
                ..Default::default()
            }],
            ..Default::default()
        })
        .encode_to_vec(),
    )
    .expect("split apk introduction");
    assert_eq!(offer.kind(), OfferKind::AndroidApp);
    assert_eq!(offer.file_count(), 2);
    assert_eq!(offer.name(), "base.apk");
    assert_eq!(offer.size_bytes(), 24);
    assert_eq!(offer.payload_id(), 11);
    assert_eq!(offer.package_name(), Some("dev.chat"));
    let split = offer.file(1).expect("config split");
    assert_eq!(split.name(), "config.apk");
    assert_eq!(split.size_bytes(), 8);
    assert_eq!(split.payload_id(), 12);
}

#[test]
fn rejects_misaligned_split_apk_introduction() {
    let frame = introduction_frame(IntroductionFrame {
        app_metadata: vec![AppMetadata {
            size: Some(16),
            payload_id: vec![11, 12],
            file_name: vec![String::from("base.apk")],
            file_size: vec![16],
            ..Default::default()
        }],
        ..Default::default()
    });
    assert!(matches!(
        SharingSession::decode_offer(&frame.encode_to_vec()),
        Err(ProtocolError::InvalidOffer(_))
    ));
}

#[test]
fn outbound_tcp_connect_establishes_encryption_with_a_loopback_peer() {
    let (listener, address) = bind_loopback();
    let responder = thread::spawn(move || {
        let mut session =
            SharingSession::accept(accept_stream(listener), "remote", "Remote")
                .expect("establish peer session");
        pair_unable(&mut session);
    });

    let mut session =
        SharingSession::connect(connect_stream(address), "local", "Omarchy")
            .expect("establish local session");
    pair_unable(&mut session);
    responder.join().expect("responder completes");
}

#[test]
fn account_free_pairing_chunks_one_file_across_connection_events() {
    let bytes = vec![0xA5; MULTI_FRAME_FILE_SIZE];
    let expected = bytes.clone();
    let (listener, address) = bind_loopback();
    let receiver = spawn_peer(listener, move |mut session| {
        assert_eq!(session.verification_code(), "9418");
        pair_unable(&mut session);
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.name(), "note.txt");
        session.accept_incoming_offer().expect("accept offer");
        let mut received = Vec::new();
        let mut last_progress = 0_u64;
        session
            .receive_incoming_file(
                &offer,
                &mut received,
                |transferred| {
                    assert!(transferred >= last_progress);
                    last_progress = transferred;
                },
                || false,
            )
            .expect("receive file");
        assert_eq!(received, expected);
        assert_eq!(
            last_progress,
            u64::try_from(MULTI_FRAME_FILE_SIZE).expect("file size")
        );
    });

    let mut session = connect_session(address);
    assert_eq!(session.verification_code(), "9418");
    pair_unable(&mut session);
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
fn receiver_rejection_reaches_the_outbound_sender() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.reject_incoming_offer().expect("reject offer");
    });
    let error = session
        .send_outgoing_file(
            "rejected.txt",
            1,
            &mut Cursor::new([1_u8]),
            || {},
            |_| {},
            || false,
        )
        .expect_err("peer rejection");
    assert!(matches!(error, ProtocolError::Rejected));
    receiver.join().expect("receiver completes");
}

#[test]
fn outbound_cancellation_reaches_the_receiver_between_file_chunks() {
    let bytes = vec![0xA5; MULTI_FRAME_FILE_SIZE];
    let (mut session, receiver) = paired_loopback(|mut session| {
        let offer = session.receive_incoming_offer().expect("receive offer");
        session.accept_incoming_offer().expect("accept offer");
        let mut received = Vec::new();
        let error = session
            .receive_incoming_file(&offer, &mut received, |_| {}, || false)
            .expect_err("sender cancellation");
        assert!(matches!(error, ProtocolError::Cancelled));
        assert_eq!(received.len(), 0x0001_0000);
    });
    let cancellation_checks = Cell::new(0_u8);
    let error = session
        .send_outgoing_file(
            "cancelled.txt",
            u64::try_from(MULTI_FRAME_FILE_SIZE).expect("file size"),
            &mut Cursor::new(bytes),
            || {},
            |_| {},
            || {
                let checks = cancellation_checks.get();
                cancellation_checks.set(checks.saturating_add(1));
                checks == 3
            },
        )
        .expect_err("local cancellation");
    assert!(matches!(error, ProtocolError::Cancelled));
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
