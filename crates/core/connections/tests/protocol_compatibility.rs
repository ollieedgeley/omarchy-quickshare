//! Reference-aligned encrypted frame compatibility contracts.

#![expect(
    clippy::as_conversions,
    clippy::big_endian_bytes,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::tests_outside_test_module,
    reason = "The manual peer exposes protocol frames at the public seam"
)]

use core::time::Duration;
use payload_transfer_frame::control_message::EventType as ControlEvent;
use prost::Message as _;
use quickshare_connections::{Connection, ConnectionOptions, Error, Event};
use quickshare_crypto::Handshake;
use quickshare_wire::connections::{
    BandwidthUpgradeRetryFrame, ConnectionRequestFrame,
    ConnectionResponseFrame, DisconnectionFrame, KeepAliveFrame, OfflineFrame,
    PayloadTransferFrame, V1Frame, bandwidth_upgrade_retry_frame,
    connection_response_frame, offline_frame, payload_transfer_frame, v1_frame,
};
use rand_core as _;
use rustix as _;
use std::{
    io::{self, Read as _, Write as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    thread,
};
use tracing as _;

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

#[test]
fn responder_ignores_reference_retry_request_and_receives_following_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, false);
        write_encrypted(&mut stream, &mut channel, &retry_request(), [7; 16]);

        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("bound unexpected response read");
        let mut prefix = [0; 4];
        assert!(matches!(
            stream.read_exact(&mut prefix),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                )
        ));
        stream
            .set_read_timeout(None)
            .expect("clear response timeout");

        write_encrypted(
            &mut stream,
            &mut channel,
            &data_frame(7, b"after retry"),
            [8; 16],
        );
    });

    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder encryption");
    assert_eq!(
        connection.receive().expect("receive payload after retry"),
        Event::Bytes {
            id: 7,
            bytes: b"after retry".to_vec(),
        }
    );
    peer.join().expect("manual peer completes");
}

#[test]
fn initiator_ignores_reference_retry_request_and_receives_following_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept initiator");
        let mut channel = respond(&mut stream, false);
        write_encrypted(&mut stream, &mut channel, &retry_request(), [7; 16]);
        write_encrypted(
            &mut stream,
            &mut channel,
            &data_frame(8, b"after retry"),
            [8; 16],
        );
    });

    let stream = TcpStream::connect(address).expect("connect manual peer");
    let mut connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator"),
    )
    .expect("establish initiator encryption");
    assert_eq!(
        connection.receive().expect("receive payload after retry"),
        Event::Bytes {
            id: 8,
            bytes: b"after retry".to_vec(),
        }
    );
    peer.join().expect("manual peer completes");
}

#[test]
fn post_transfer_drain_handles_late_retry_keepalive_and_peer_close() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, false);
        write_encrypted(&mut stream, &mut channel, &retry_request(), [7; 16]);
        write_encrypted(
            &mut stream,
            &mut channel,
            &keepalive_frame(false, 43),
            [8; 16],
        );
        let acknowledged = channel
            .decrypt(&read_frame(&mut stream))
            .expect("decrypt keepalive acknowledgement");
        assert_eq!(
            OfflineFrame::decode(acknowledged.as_slice())
                .expect("decode keepalive acknowledgement"),
            keepalive_frame(true, 43)
        );
    });

    let (stream, _) = listener.accept().expect("accept manual peer");
    let connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder encryption");
    connection
        .drain_post_transfer_control(Duration::from_secs(1), || false)
        .expect("drain late authenticated control");
    peer.join().expect("manual peer completes");
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The manual peer keeps retired control ordering visible"
)]
fn post_transfer_drain_ignores_retired_control_before_keepalive() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, false);
        drop(
            channel
                .decrypt(&read_frame(&mut stream))
                .expect("first DATA"),
        );
        drop(
            channel
                .decrypt(&read_frame(&mut stream))
                .expect("last DATA"),
        );
        write_encrypted(
            &mut stream,
            &mut channel,
            &control_event_frame(71, 3, 3, ControlEvent::PayloadCanceled),
            [7; 16],
        );
        write_encrypted(
            &mut stream,
            &mut channel,
            &control_event_frame(72, 3, 3, ControlEvent::PayloadError),
            [8; 16],
        );
        write_encrypted(
            &mut stream,
            &mut channel,
            &payload_ack_frame(999),
            [9; 16],
        );
        write_encrypted(
            &mut stream,
            &mut channel,
            &keepalive_frame(false, 45),
            [10; 16],
        );
        let acknowledged = channel
            .decrypt(&read_frame(&mut stream))
            .expect("decrypt keepalive acknowledgement");
        assert_eq!(
            OfflineFrame::decode(acknowledged.as_slice()).expect("decode ACK"),
            keepalive_frame(true, 45)
        );
    });

    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder encryption");
    connection
        .send_bytes(71, b"sent")
        .expect("send local payload");
    connection
        .drain_post_transfer_control(Duration::from_secs(1), || false)
        .expect("ignore retired payload controls");
    peer.join().expect("manual peer completes");
}
#[test]
fn file_sender_stops_after_peer_cancels_between_chunks() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let (control_sender, control_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, false);
        drop(
            channel
                .decrypt(&read_frame(&mut stream))
                .expect("first FILE chunk"),
        );
        write_encrypted(
            &mut stream,
            &mut channel,
            &control_event_frame(73, 3, 1, ControlEvent::PayloadCanceled),
            [7; 16],
        );
        control_sender.send(()).expect("control written");
        release_receiver.recv().expect("release peer");
        let mut next = [0_u8; 1];
        assert_eq!(
            stream.read(&mut next).expect("observe sender closure"),
            0,
            "sender must not write a terminal chunk after cancellation"
        );
    });
    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder encryption");
    connection
        .send_file_header(73, 3, Some("note.txt".into()))
        .expect("declare FILE");
    connection
        .send_file_chunk(73, 0, b"a", false)
        .expect("send first chunk");
    control_receiver.recv().expect("peer control arrived");
    let result = connection.send_file_chunk(73, 1, b"bc", true);
    drop(connection);
    release_sender.send(()).expect("release peer");
    peer.join().expect("manual peer completes");
    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn negative_payload_control_offset_is_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, false);
        write_encrypted(
            &mut stream,
            &mut channel,
            &control_frame(23, -1),
            [7; 16],
        );
    });

    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder encryption");
    assert!(matches!(connection.receive(), Err(Error::InvalidPayload)));
    peer.join().expect("manual peer completes");
}

#[test]
fn file_payload_rejects_a_nonzero_initial_offset() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, false);
        write_encrypted(
            &mut stream,
            &mut channel,
            &file_data_frame(29, 1, b"abc", true),
            [7; 16],
        );
    });

    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder encryption");
    assert!(matches!(connection.receive(), Err(Error::InvalidPayload)));
    peer.join().expect("manual peer completes");
}

#[test]
fn responder_echoes_keepalive_while_waiting_for_connection_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind responder");
    let address = listener.local_addr().expect("responder address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        let mut channel = initiate(&mut stream, true);
        write_encrypted(
            &mut stream,
            &mut channel,
            &data_frame(31, b"responder ready"),
            [7; 16],
        );
    });
    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut responder = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder after keepalive");
    assert!(matches!(
        responder.receive(),
        Ok(Event::Bytes { id: 31, .. })
    ));
    peer.join().expect("initiating peer completes");
}

#[test]
fn initiator_echoes_keepalive_while_waiting_for_connection_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind initiator");
    let address = listener.local_addr().expect("initiator address");
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept initiator");
        let mut channel = respond(&mut stream, true);
        write_encrypted(
            &mut stream,
            &mut channel,
            &data_frame(32, b"initiator ready"),
            [7; 16],
        );
    });
    let stream = TcpStream::connect(address).expect("connect manual peer");
    let mut initiator = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator"),
    )
    .expect("establish initiator after keepalive");
    assert!(matches!(
        initiator.receive(),
        Ok(Event::Bytes { id: 32, .. })
    ));
    peer.join().expect("responding peer completes");
}

#[test]
fn both_roles_stop_if_peer_disconnects_while_waiting_for_response() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind responder");
    let address = listener.local_addr().expect("responder address");
    let peer = thread::spawn(move || {
        let mut stream =
            TcpStream::connect(address).expect("connect responder");
        initiate_disconnect(&mut stream);
    });
    let (stream, _) = listener.accept().expect("accept manual peer");
    assert!(matches!(
        Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder"),
        ),
        Err(Error::Rejected)
    ));
    peer.join().expect("initiating peer completes");

    let initiator_listener =
        TcpListener::bind("127.0.0.1:0").expect("bind initiator");
    let initiator_address =
        initiator_listener.local_addr().expect("initiator address");
    let responding_peer = thread::spawn(move || {
        let (mut accepted_stream, _) =
            initiator_listener.accept().expect("accept initiator");
        respond_disconnect(&mut accepted_stream);
    });
    let initiator_stream =
        TcpStream::connect(initiator_address).expect("connect manual peer");
    assert!(matches!(
        Connection::connect(
            initiator_stream,
            Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
            ConnectionOptions::new("initiator", "initiator"),
        ),
        Err(Error::Rejected)
    ));
    responding_peer.join().expect("responding peer completes");
}

#[expect(clippy::single_call_fn, reason = "Named manual peer role")]
fn initiate_disconnect(stream: &mut TcpStream) {
    write_frame(stream, &request_frame().encode_to_vec());
    let mut handshake =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    write_frame(
        stream,
        &handshake.next_message().expect("create raw UKEY2 M1"),
    );
    handshake
        .receive(&read_frame(stream))
        .expect("receive raw UKEY2 M2");
    write_frame(
        stream,
        &handshake.next_message().expect("create raw UKEY2 M3"),
    );
    assert_eq!(
        OfflineFrame::decode(read_frame(stream).as_slice())
            .expect("decode plaintext Connections ACCEPT"),
        accept_frame()
    );
    write_frame(stream, &disconnection_frame().encode_to_vec());
}

#[expect(clippy::single_call_fn, reason = "Named manual peer role")]
fn respond_disconnect(stream: &mut TcpStream) {
    assert_eq!(
        OfflineFrame::decode(read_frame(stream).as_slice())
            .expect("decode plaintext Connections request"),
        request_frame()
    );
    let mut handshake =
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);
    handshake
        .receive(&read_frame(stream))
        .expect("receive raw UKEY2 M1");
    write_frame(
        stream,
        &handshake.next_message().expect("create raw UKEY2 M2"),
    );
    handshake
        .receive(&read_frame(stream))
        .expect("receive raw UKEY2 M3");
    assert_eq!(
        OfflineFrame::decode(read_frame(stream).as_slice())
            .expect("decode plaintext Connections ACCEPT"),
        accept_frame()
    );
    write_frame(stream, &disconnection_frame().encode_to_vec());
}

fn initiate(
    stream: &mut TcpStream,
    preconfirmation_keepalive: bool,
) -> quickshare_crypto::SecureChannel {
    write_frame(stream, &request_frame().encode_to_vec());
    let mut handshake =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    write_frame(
        stream,
        &handshake.next_message().expect("create raw UKEY2 M1"),
    );
    handshake
        .receive(&read_frame(stream))
        .expect("receive raw UKEY2 M2");
    write_frame(
        stream,
        &handshake.next_message().expect("create raw UKEY2 M3"),
    );
    assert_eq!(
        OfflineFrame::decode(read_frame(stream).as_slice())
            .expect("decode plaintext Connections ACCEPT"),
        accept_frame()
    );
    if preconfirmation_keepalive {
        write_frame(stream, &keepalive_frame(false, 41).encode_to_vec());
    }
    write_frame(stream, &accept_frame().encode_to_vec());
    if preconfirmation_keepalive {
        assert_eq!(
            OfflineFrame::decode(read_frame(stream).as_slice())
                .expect("decode preconfirmation keepalive ACK"),
            keepalive_frame(true, 41)
        );
    }
    handshake.complete().expect("complete UKEY2").into_channel()
}

fn respond(
    stream: &mut TcpStream,
    preconfirmation_keepalive: bool,
) -> quickshare_crypto::SecureChannel {
    let request = OfflineFrame::decode(read_frame(stream).as_slice())
        .expect("decode plaintext Connections request");
    assert_eq!(request, request_frame());
    let mut handshake =
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);
    handshake
        .receive(&read_frame(stream))
        .expect("receive raw UKEY2 M1");
    write_frame(
        stream,
        &handshake.next_message().expect("create raw UKEY2 M2"),
    );
    handshake
        .receive(&read_frame(stream))
        .expect("receive raw UKEY2 M3");
    assert_eq!(
        OfflineFrame::decode(read_frame(stream).as_slice())
            .expect("decode plaintext Connections ACCEPT"),
        accept_frame()
    );
    if preconfirmation_keepalive {
        write_frame(stream, &keepalive_frame(false, 42).encode_to_vec());
    }
    write_frame(stream, &accept_frame().encode_to_vec());
    if preconfirmation_keepalive {
        assert_eq!(
            OfflineFrame::decode(read_frame(stream).as_slice())
                .expect("decode preconfirmation keepalive ACK"),
            keepalive_frame(true, 42)
        );
    }
    handshake.complete().expect("complete UKEY2").into_channel()
}

fn retry_request() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::BandwidthUpgradeRetry as i32),
        bandwidth_upgrade_retry: Some(BandwidthUpgradeRetryFrame {
            supported_medium: vec![
                bandwidth_upgrade_retry_frame::Medium::Bluetooth as i32,
                bandwidth_upgrade_retry_frame::Medium::WifiHotspot as i32,
                bandwidth_upgrade_retry_frame::Medium::Ble as i32,
                bandwidth_upgrade_retry_frame::Medium::WifiLan as i32,
                bandwidth_upgrade_retry_frame::Medium::WifiDirect as i32,
            ],
            is_request: Some(true),
        }),
        ..Default::default()
    })
}

fn keepalive_frame(ack: bool, sequence_number: u32) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::KeepAlive as i32),
        keep_alive: Some(KeepAliveFrame {
            ack: Some(ack),
            seq_num: Some(sequence_number),
        }),
        ..Default::default()
    })
}

fn disconnection_frame() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::Disconnection as i32),
        disconnection: Some(DisconnectionFrame::default()),
        ..Default::default()
    })
}

fn request_frame() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::ConnectionRequest as i32),
        connection_request: Some(ConnectionRequestFrame {
            endpoint_id: Some("initiator".into()),
            endpoint_name: Some(b"initiator".to_vec()),
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

fn data_frame(id: i64, bytes: &[u8]) -> OfflineFrame {
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
                total_size: Some(
                    i64::try_from(bytes.len()).expect("payload fits i64"),
                ),
                ..Default::default()
            }),
            payload_chunk: Some(payload_transfer_frame::PayloadChunk {
                flags: Some(
                    payload_transfer_frame::payload_chunk::Flags::LastChunk
                        as i32,
                ),
                offset: Some(0),
                body: Some(bytes.to_vec()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

#[expect(clippy::single_call_fn, reason = "Named malformed frame fixture")]
fn file_data_frame(
    id: i64,
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
                    payload_transfer_frame::payload_header::PayloadType::File
                        as i32,
                ),
                total_size: Some(3),
                file_name: Some("note.txt".into()),
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

#[expect(clippy::single_call_fn, reason = "Named malformed frame fixture")]
fn control_frame(id: i64, offset: i64) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::PayloadTransfer as i32),
        payload_transfer: Some(PayloadTransferFrame {
            packet_type: Some(
                payload_transfer_frame::PacketType::Control as i32,
            ),
            payload_header: Some(payload_transfer_frame::PayloadHeader {
                id: Some(id),
                total_size: Some(3),
                ..Default::default()
            }),
            control_message: Some(payload_transfer_frame::ControlMessage {
                event: Some(
                    payload_transfer_frame::control_message::EventType::
                        PayloadError as i32,
                ),
                offset: Some(offset),
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn control_event_frame(
    id: i64,
    total_size: i64,
    offset: i64,
    event: ControlEvent,
) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::PayloadTransfer as i32),
        payload_transfer: Some(PayloadTransferFrame {
            packet_type: Some(
                payload_transfer_frame::PacketType::Control as i32,
            ),
            payload_header: Some(payload_transfer_frame::PayloadHeader {
                id: Some(id),
                total_size: Some(total_size),
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

#[expect(clippy::single_call_fn, reason = "Named retired frame fixture")]
fn payload_ack_frame(id: i64) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::PayloadTransfer as i32),
        payload_transfer: Some(PayloadTransferFrame {
            packet_type: Some(
                payload_transfer_frame::PacketType::PayloadAck as i32,
            ),
            payload_header: Some(payload_transfer_frame::PayloadHeader {
                id: Some(id),
                ..Default::default()
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

fn write_encrypted(
    stream: &mut TcpStream,
    channel: &mut quickshare_crypto::SecureChannel,
    frame: &OfflineFrame,
    iv: [u8; 16],
) {
    let encrypted = channel
        .encrypt(&frame.encode_to_vec(), iv)
        .expect("encrypt offline frame");
    write_frame(stream, &encrypted);
}

fn write_frame(stream: &mut TcpStream, bytes: &[u8]) {
    stream
        .write_all(
            &u32::try_from(bytes.len())
                .expect("frame fits u32")
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
