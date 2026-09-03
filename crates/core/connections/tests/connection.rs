//! Loopback contracts for the public Connections seam.

#![expect(
    clippy::as_conversions,
    clippy::big_endian_bytes,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::single_call_fn,
    clippy::tests_outside_test_module,
    reason = "The manual peer exposes protocol frames for contract assertions"
)]

use prost::Message as _;
use quickshare_connections::{Connection, ConnectionOptions, Event};
use quickshare_crypto::Handshake;
use quickshare_wire::{
    connections::{
        ConnectionRequestFrame, ConnectionResponseFrame, OfflineFrame,
        PayloadTransferFrame, V1Frame, connection_response_frame,
        offline_frame, payload_transfer_frame, v1_frame,
    },
    sharing::Frame,
};
use rand_core as _;
use std::thread;
use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
};

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The manual peer keeps the initiating-side wire trace visible."
)]
fn initiating_connection_exchanges_plain_accepts_before_encrypted_data() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept initiator");
        let request = OfflineFrame::decode(read_frame(&mut stream).as_slice())
            .expect("decode plaintext request");
        assert_eq!(
            request.v1.and_then(|frame| frame.connection_request),
            Some(ConnectionRequestFrame {
                endpoint_id: Some("initiator".into()),
                endpoint_info: Some(b"sharing-advertisement".to_vec()),
                endpoint_name: Some("initiator".into()),
                handshake_data: None,
                ..Default::default()
            })
        );

        let mut handshake =
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);
        handshake
            .receive(&read_frame(&mut stream))
            .expect("receive raw UKEY2 M1");
        write_frame(
            &mut stream,
            &handshake.next_message().expect("create raw UKEY2 M2"),
        );
        handshake
            .receive(&read_frame(&mut stream))
            .expect("receive raw UKEY2 M3");
        write_frame(&mut stream, &accept_frame().encode_to_vec());
        assert_eq!(
            OfflineFrame::decode(read_frame(&mut stream).as_slice())
                .expect("decode plaintext Connections ACCEPT"),
            accept_frame()
        );
        let mut channel =
            handshake.complete().expect("complete UKEY2").into_channel();

        let data = channel
            .decrypt(&read_frame(&mut stream))
            .expect("decrypt DATA frame");
        assert_data(
            OfflineFrame::decode(data.as_slice()).expect("decode DATA frame"),
            7,
            payload_transfer_frame::payload_header::PayloadType::Bytes,
            b"outbound bytes",
            true,
        );
    });

    let stream = TcpStream::connect(address).expect("connect peer");
    let mut connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_endpoint_info(b"sharing-advertisement".to_vec()),
    )
    .expect("establish encryption against manual peer");
    assert_eq!(connection.verification_code(), "9418");
    connection
        .send_bytes(7, b"outbound bytes")
        .expect("send bytes after ACCEPT");

    peer.join().expect("manual peer completes");
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The manual peer keeps the accepting-side wire trace visible."
)]
fn accepting_connection_receives_header_and_chunk_from_one_data_frame() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind peer");
    let address = listener.local_addr().expect("peer address");
    let peer = thread::spawn(move || {
        let mut stream = TcpStream::connect(address).expect("connect acceptor");
        write_frame(&mut stream, &request_frame().encode_to_vec());

        let mut handshake =
            Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
        write_frame(
            &mut stream,
            &handshake.next_message().expect("create raw UKEY2 M1"),
        );
        handshake
            .receive(&read_frame(&mut stream))
            .expect("receive raw UKEY2 M2");
        write_frame(
            &mut stream,
            &handshake.next_message().expect("create raw UKEY2 M3"),
        );
        write_frame(&mut stream, &accept_frame().encode_to_vec());
        assert_eq!(
            OfflineFrame::decode(read_frame(&mut stream).as_slice())
                .expect("decode plaintext Connections ACCEPT"),
            accept_frame()
        );
        let mut channel =
            handshake.complete().expect("complete UKEY2").into_channel();
        write_frame(
            &mut stream,
            &channel
                .encrypt(
                    &data_frame(
                        8,
                        payload_transfer_frame::payload_header::
                            PayloadType::Bytes,
                        3,
                        None,
                        0,
                        b"abc",
                        false,
                    )
                    .encode_to_vec(),
                    [7; 16],
                )
                .expect("encrypt BYTES body"),
        );
        write_frame(
            &mut stream,
            &channel
                .encrypt(
                    &data_frame(
                        8,
                        payload_transfer_frame::payload_header::
                            PayloadType::Bytes,
                        3,
                        None,
                        3,
                        b"",
                        true,
                    )
                    .encode_to_vec(),
                    [8; 16],
                )
                .expect("encrypt BYTES terminator"),
        );
        write_frame(
            &mut stream,
            &channel
                .encrypt(
                    &data_frame(
                        9,
                        payload_transfer_frame::payload_header::
                            PayloadType::File,
                        3,
                        Some("note.txt".into()),
                        0,
                        b"abc",
                        true,
                    )
                    .encode_to_vec(),
                    [9; 16],
                )
                .expect("encrypt DATA frame"),
        );
    });

    let (stream, _) = listener.accept().expect("accept manual peer");
    let mut connection = Connection::accept(
        stream,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("acceptor", "acceptor"),
    )
    .expect("establish encryption with manual peer");
    assert_eq!(
        connection.receive().expect("receive split BYTES payload"),
        Event::Bytes {
            id: 8,
            bytes: b"abc".to_vec(),
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
        connection.receive().expect("receive preserved file chunk"),
        Event::FileChunk {
            id: 9,
            offset: 0,
            bytes: b"abc".to_vec(),
            is_last: true,
        }
    );

    peer.join().expect("manual peer completes");
}

fn request_frame() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::ConnectionRequest as i32),
        connection_request: Some(ConnectionRequestFrame {
            endpoint_id: Some("initiator".into()),
            endpoint_name: Some(vec![0xff, 0xfe]),
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

fn data_frame(
    id: i64,
    kind: payload_transfer_frame::payload_header::PayloadType,
    size: i64,
    name: Option<String>,
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
                r#type: Some(kind as i32),
                total_size: Some(size),
                file_name: name,
                ..Default::default()
            }),
            payload_chunk: Some(payload_transfer_frame::PayloadChunk {
                flags: Some(if last {
                    payload_transfer_frame::payload_chunk::Flags::LastChunk
                        as i32
                } else {
                    0_i32
                }),
                offset: Some(offset),
                body: (!bytes.is_empty()).then(|| bytes.to_vec()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn assert_data(
    frame: OfflineFrame,
    id: i64,
    kind: payload_transfer_frame::payload_header::PayloadType,
    bytes: &[u8],
    last: bool,
) {
    let size = i64::try_from(bytes.len()).expect("payload size fits i64");
    let transfer = frame
        .v1
        .and_then(|value| value.payload_transfer)
        .expect("DATA transfer");
    assert_eq!(
        transfer.packet_type,
        Some(payload_transfer_frame::PacketType::Data as i32)
    );
    assert_eq!(
        transfer.payload_header,
        data_frame(id, kind, size, None, 0, bytes, last)
            .v1
            .and_then(|value| value.payload_transfer)
            .and_then(|value| value.payload_header)
    );
    assert_eq!(
        transfer.payload_chunk,
        data_frame(id, kind, size, None, 0, bytes, last)
            .v1
            .and_then(|value| value.payload_transfer)
            .and_then(|value| value.payload_chunk)
    );
}

const fn offline(v1: V1Frame) -> OfflineFrame {
    OfflineFrame {
        version: Some(offline_frame::Version::V1 as i32),
        v1: Some(v1),
    }
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
#[expect(
    clippy::too_many_lines,
    reason = "The test keeps both TCP roles in one observable loopback trace."
)]
fn loopback_connection_encrypts_sharing_bytes_and_file_chunks() {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
    let address = listener.local_addr().expect("listener address");
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept connection");
        let mut connection = Connection::accept(
            stream,
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
        connection.send_keepalive(4).expect("send keepalive");
        assert_eq!(
            connection
                .receive()
                .expect("receive keepalive acknowledgement"),
            Event::KeepAlive {
                ack: true,
                sequence: 4,
            }
        );
        connection.disconnect().expect("send disconnection");
    });

    let stream = TcpStream::connect(address).expect("connect loopback");
    let mut connection = Connection::connect(
        stream,
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
        connection.receive().expect("receive keepalive"),
        Event::KeepAlive {
            ack: false,
            sequence: 4,
        }
    );
    assert_eq!(
        connection.receive().expect("receive disconnect"),
        Event::Disconnected
    );

    responder.join().expect("responder completes");
}
