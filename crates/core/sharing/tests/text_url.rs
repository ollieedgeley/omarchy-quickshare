//! Public text and URL Sharing protocol contracts.

#![expect(
    clippy::as_conversions,
    clippy::big_endian_bytes,
    clippy::absolute_paths,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::panic_in_result_fn,
    clippy::single_call_fn,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    clippy::unwrap_in_result,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

mod common;

use base64 as _;
use common::{
    INITIATOR_RANDOM, INITIATOR_SECRET, bind_loopback, connect_stream,
    decode_google_offer, google_v1, paired_loopback, spawn_peer,
};
use prost::Message as _;
use quickshare_connections::{Connection, ConnectionOptions, Event};
use quickshare_crypto::{Handshake, SecureChannel};
use quickshare_sharing::{
    IncomingOffer, OfferKind, ProtocolError, SharingSession,
};
use quickshare_wire::{
    connections::{
        ConnectionRequestFrame, ConnectionResponseFrame, OfflineFrame,
        PayloadTransferFrame, V1Frame, connection_response_frame,
        offline_frame, payload_transfer_frame, v1_frame,
    },
    sharing::connection_response_frame as response,
};
use rand_core as _;
use serde as _;
use std::{
    io::{Read as _, Write as _},
    net::TcpStream,
};
use tracing as _;
use tracing_subscriber as _;

fn assert_offer(
    offer: &IncomingOffer,
    kind: OfferKind,
    name: &str,
    size: i64,
    payload_id: i64,
) {
    assert_eq!(offer.kind(), kind);
    assert_eq!(offer.name(), name);
    assert_eq!(offer.size_bytes(), size);
    assert_eq!(offer.payload_id(), payload_id);
}

fn accept_named_offer(
    session: &mut SharingSession,
    kind: OfferKind,
    name: &str,
) -> IncomingOffer {
    let offer = session.receive_incoming_offer().expect("receive offer");
    assert_eq!(offer.kind(), kind);
    assert_eq!(offer.name(), name);
    session.accept_incoming_offer().expect("accept offer");
    offer
}

#[test]
fn decodes_google_text_and_url_introductions() {
    assert_offer(
        &decode_google_offer(
            google_v1!("incoming/introductions/text.bin"),
            "Google incoming text introduction",
        ),
        OfferKind::Text,
        "fixture text",
        12,
        202,
    );

    assert_offer(
        &decode_google_offer(
            google_v1!("incoming/introductions/url.bin"),
            "Google incoming URL introduction",
        ),
        OfferKind::Url,
        "https://x.y",
        11,
        203,
    );

    assert_offer(
        &decode_google_offer(
            google_v1!("outgoing/introductions/text.bin"),
            "Google outgoing text introduction",
        ),
        OfferKind::Text,
        "fixture text",
        12,
        400,
    );

    assert_offer(
        &decode_google_offer(
            google_v1!("outgoing/introductions/url.bin"),
            "Google outgoing URL introduction",
        ),
        OfferKind::Url,
        "https://x.y",
        11,
        400,
    );
}

#[test]
fn accepts_google_apk_introduction_as_a_file() {
    let offer = decode_google_offer(
        google_v1!("incoming/introductions/apk.bin"),
        "Google APK introduction",
    );
    assert_eq!(offer.kind(), OfferKind::AndroidApp);
    assert!(offer.kind().persists_as_file());
    assert_eq!(offer.name(), "FixtureApp.apk");
    assert_eq!(offer.size_bytes(), 16);
    assert_eq!(offer.payload_id(), 204);
    assert_eq!(offer.package_name(), Some("dev.fixture.app"));
}

#[test]
fn loopback_sends_plain_text_after_consent() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let offer = accept_named_offer(
            &mut session,
            OfferKind::Text,
            "hello from omarchy",
        );
        assert_eq!(
            session
                .receive_incoming_text(&offer, |_| {}, || false)
                .expect("receive text"),
            "hello from omarchy"
        );
    });
    session
        .send_outgoing_text("hello from omarchy", || {}, |_| {}, || false)
        .expect("send text after accept");
    receiver.join().expect("receiver completes");
}

#[test]
fn loopback_sends_url_after_consent() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let offer = accept_named_offer(
            &mut session,
            OfferKind::Url,
            "https://omarchy.local",
        );
        assert_eq!(
            session
                .receive_incoming_url(&offer, |_| {}, || false)
                .expect("receive url"),
            "https://omarchy.local"
        );
    });
    session
        .send_outgoing_url("https://omarchy.local", || {}, |_| {}, || false)
        .expect("send url after accept");
    receiver.join().expect("receiver completes");
}

#[test]
fn text_rejection_reaches_the_outbound_sender() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.reject_incoming_offer().expect("reject offer");
    });
    let error = session
        .send_outgoing_text("rejected text", || {}, |_| {}, || false)
        .expect_err("peer rejection");
    assert!(matches!(error, ProtocolError::Rejected));
    receiver.join().expect("receiver completes");
}

#[test]
fn keepalive_before_text_introduction_is_ignored() {
    let (mut session, receiver) = paired_loopback(|mut session| {
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
    session.send_keepalive(1).expect("send keepalive");
    session
        .send_outgoing_text("ping", || {}, |_| {}, || false)
        .expect("send text after keepalive");
    receiver.join().expect("receiver completes");
}

fn receive_google_text_payloads(
    payloads: &[(i64, &[u8])],
) -> Result<String, ProtocolError> {
    let (listener, address) = bind_loopback();
    let receiver = spawn_peer(listener, |mut session| {
        let offer =
            accept_named_offer(&mut session, OfferKind::Text, "fixture text");
        session.receive_incoming_text(&offer, |_| {}, || false)
    });
    let mut sender = Connection::connect(
        connect_stream(address),
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish sender connection");

    sender
        .send_bytes(101, google_v1!("incoming/introductions/text.bin"))
        .expect("send Google text introduction");
    let Event::Bytes {
        bytes: acceptance_bytes,
        ..
    } = sender.receive()?
    else {
        return Err(ProtocolError::InvalidPayload);
    };
    assert_eq!(
        SharingSession::decode_response(&acceptance_bytes)
            .expect("decode acceptance"),
        response::Status::Accept
    );
    for &(id, payload_bytes) in payloads {
        sender
            .send_bytes(id, payload_bytes)
            .expect("send scripted payload");
    }
    receiver.join().expect("receiver completes")
}

#[test]
fn inbound_text_ignores_a_google_control_frame_before_its_payload() {
    assert_eq!(
        receive_google_text_payloads(&[
            (102, google_v1!("incoming/responses/accept.bin")),
            (202, b"fixture text"),
        ])
        .expect("receive introduced text"),
        "fixture text"
    );
}

#[test]
fn inbound_text_does_not_treat_payload_data_as_control() {
    assert!(matches!(
        receive_google_text_payloads(&[(102, b"fixture text")]),
        Err(ProtocolError::InvalidPayload)
    ));
    assert!(matches!(
        receive_google_text_payloads(&[(202, b"short")]),
        Err(ProtocolError::InvalidPayload)
    ));
}

#[test]
fn inbound_text_honors_a_google_cancel_before_its_payload() {
    assert!(matches!(
        receive_google_text_payloads(&[(
            102,
            google_v1!("incoming/responses/cancel.bin"),
        )]),
        Err(ProtocolError::Cancelled)
    ));
}

fn receive_google_text_after_statuses(
    statuses: &[(i64, payload_transfer_frame::control_message::EventType)],
) -> Result<String, ProtocolError> {
    let (listener, address) = bind_loopback();
    let receiver = spawn_peer(listener, |mut session| {
        let offer =
            accept_named_offer(&mut session, OfferKind::Text, "fixture text");
        session.receive_incoming_text(&offer, |_| {}, || false)
    });
    let mut stream = TcpStream::connect(address).expect("connect raw peer");
    write_frame(&mut stream, &request_frame().encode_to_vec());
    let mut handshake =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    write_frame(
        &mut stream,
        &handshake.next_message().expect("create UKEY2 M1"),
    );
    handshake
        .receive(&read_frame(&mut stream))
        .expect("receive UKEY2 M2");
    write_frame(
        &mut stream,
        &handshake.next_message().expect("create UKEY2 M3"),
    );
    write_frame(&mut stream, &accept_frame().encode_to_vec());
    assert_eq!(
        OfflineFrame::decode(read_frame(&mut stream).as_slice())
            .expect("decode plaintext Connections acceptance"),
        accept_frame()
    );
    let mut channel =
        handshake.complete().expect("complete UKEY2").into_channel();

    send_bytes(
        &mut stream,
        &mut channel,
        101,
        google_v1!("incoming/introductions/text.bin"),
        7,
    );
    let _accept_body = channel
        .decrypt(&read_frame(&mut stream))
        .expect("decrypt Sharing acceptance");
    let _accept_terminal = channel
        .decrypt(&read_frame(&mut stream))
        .expect("decrypt acceptance terminal");
    for (index, &(id, event)) in statuses.iter().enumerate() {
        send_encrypted(
            &mut stream,
            &mut channel,
            &control_frame(id, event, 0),
            u8::try_from(index).unwrap_or(0).saturating_add(9),
        );
    }
    if statuses.iter().all(|&(id, _)| id != 202) {
        send_bytes(&mut stream, &mut channel, 202, b"fixture text", 20);
    }
    receiver.join().expect("receiver completes")
}

#[test]
fn inbound_text_scopes_lower_payload_statuses_to_its_introduced_id() {
    use payload_transfer_frame::control_message::EventType;

    assert_eq!(
        receive_google_text_after_statuses(&[
            (999, EventType::PayloadError),
            (998, EventType::PayloadCanceled),
        ])
        .expect("unrelated statuses do not stop text"),
        "fixture text"
    );
    assert!(matches!(
        receive_google_text_after_statuses(&[(202, EventType::PayloadError)]),
        Err(ProtocolError::InvalidPayload)
    ));
    assert!(matches!(
        receive_google_text_after_statuses(&[(
            202,
            EventType::PayloadCanceled
        )]),
        Err(ProtocolError::Cancelled)
    ));
}

fn request_frame() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::ConnectionRequest as i32),
        connection_request: Some(ConnectionRequestFrame {
            endpoint_id: Some(String::from("raw")),
            endpoint_name: Some(b"Raw peer".to_vec()),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn accept_frame() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::ConnectionResponse as i32),
        connection_response: Some(ConnectionResponseFrame {
            response: Some(
                connection_response_frame::ResponseStatus::Accept as i32,
            ),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn send_bytes(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    id: i64,
    bytes: &[u8],
    iv: u8,
) {
    let size = i64::try_from(bytes.len()).expect("payload size fits i64");
    send_encrypted(stream, channel, &data_frame(id, size, 0, bytes, false), iv);
    send_encrypted(
        stream,
        channel,
        &data_frame(id, size, size, b"", true),
        iv.saturating_add(1),
    );
}

fn data_frame(
    id: i64,
    size: i64,
    offset: i64,
    bytes: &[u8],
    last: bool,
) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::PayloadTransfer as i32),
        payload_transfer: Some(PayloadTransferFrame {
            packet_type: Some(payload_transfer_frame::PacketType::Data as i32),
            payload_header: Some(payload_transfer_frame::PayloadHeader {
                id: Some(id),
                r#type: Some(
                    payload_transfer_frame::payload_header::PayloadType::Bytes
                        as i32,
                ),
                total_size: Some(size),
                ..Default::default()
            }),
            payload_chunk: Some(payload_transfer_frame::PayloadChunk {
                flags: Some(if last {
                    payload_transfer_frame::payload_chunk::Flags::LastChunk
                        as i32
                } else {
                    0
                }),
                offset: Some(offset),
                body: Some(bytes.to_vec()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn control_frame(
    id: i64,
    event: payload_transfer_frame::control_message::EventType,
    offset: i64,
) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::PayloadTransfer as i32),
        payload_transfer: Some(PayloadTransferFrame {
            packet_type: Some(
                payload_transfer_frame::PacketType::Control as i32,
            ),
            payload_header: Some(payload_transfer_frame::PayloadHeader {
                id: Some(id),
                r#type: Some(
                    payload_transfer_frame::payload_header::PayloadType::Bytes
                        as i32,
                ),
                total_size: Some(0),
                ..Default::default()
            }),
            control_message: Some(payload_transfer_frame::ControlMessage {
                event: Some(event as i32),
                offset: Some(offset),
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

const fn offline(v1: V1Frame) -> OfflineFrame {
    OfflineFrame {
        version: Some(offline_frame::Version::V1 as i32),
        v1: Some(v1),
    }
}

fn send_encrypted(
    stream: &mut TcpStream,
    channel: &mut SecureChannel,
    frame: &OfflineFrame,
    iv: u8,
) {
    let bytes = channel
        .encrypt(&frame.encode_to_vec(), [iv; 16])
        .expect("encrypt raw peer frame");
    write_frame(stream, &bytes);
}

fn write_frame(stream: &mut TcpStream, bytes: &[u8]) {
    stream
        .write_all(
            &u32::try_from(bytes.len())
                .expect("frame length fits u32")
                .to_be_bytes(),
        )
        .expect("write frame length");
    stream.write_all(bytes).expect("write frame body");
    stream.flush().expect("flush frame");
}

fn read_frame(stream: &mut TcpStream) -> Vec<u8> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).expect("read frame length");
    let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut bytes).expect("read frame body");
    bytes
}

#[test]
fn timed_out_and_unsupported_responses_are_distinct() {
    assert_eq!(
        SharingSession::decode_response(google_v1!(
            "incoming/responses/timed-out.bin"
        ))
        .expect("Google timed-out response"),
        quickshare_wire::sharing::connection_response_frame::Status::TimedOut
    );
    assert_eq!(
        SharingSession::decode_response(google_v1!(
            "incoming/responses/unsupported.bin"
        ))
        .expect("Google unsupported response"),
        response::Status::UnsupportedAttachmentType
    );

    let (mut session, receiver) = paired_loopback(|mut session| {
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.timeout_incoming_offer().expect("timeout offer");
    });
    let error = session
        .send_outgoing_text("late", || {}, |_| {}, || false)
        .expect_err("peer timeout");
    assert!(matches!(error, ProtocolError::TimedOut));
    receiver.join().expect("receiver completes");
}
