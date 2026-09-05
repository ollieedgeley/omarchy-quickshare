//! Public Connections contracts over a generic byte-stream seam.

#![expect(
    clippy::as_conversions,
    clippy::big_endian_bytes,
    clippy::expect_used,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    reason = "The manual peer exercises big-endian prost protocol framing"
)]

#[path = "stream_seam/raw_peer.rs"]
mod raw_peer;

use prost::Message as _;
use quickshare_connections::{
    Connection, ConnectionOptions, Event, Medium, UpgradeCredentials,
    UpgradeEvent, UpgradeState,
};
use quickshare_crypto::Handshake;
use quickshare_wire::{
    connections::{
        BandwidthUpgradeNegotiationFrame, ConnectionResponseFrame,
        DisconnectionFrame, KeepAliveFrame, OfflineFrame, PayloadTransferFrame,
        V1Frame, bandwidth_upgrade_negotiation_frame as bwu,
        connection_response_frame, offline_frame, payload_transfer_frame,
        v1_frame,
    },
    sharing::Frame,
};
use rand_core as _;
use raw_peer::RawPeer;
use rustix as _;
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
};
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
fn prior_channel_keepalive_does_not_interrupt_upgrade_transition() {
    let (old_initiator, old_responder) = UnixStream::pair().expect("old pair");
    let (new_initiator, new_responder) = UnixStream::pair().expect("new pair");
    let responder = thread::spawn(move || {
        let mut peer = RawPeer::accept(old_responder);
        let path = assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::UpgradePathAvailable,
        );
        assert!(path.upgrade_path_info.is_some());
        write_plain(
            &mut &new_responder,
            &upgrade_frame(
                bwu::EventType::ClientIntroduction,
                BandwidthUpgradeNegotiationFrame {
                    client_introduction: Some(bwu::ClientIntroduction {
                        endpoint_id: Some("responder".into()),
                        supports_disabling_encryption: Some(false),
                        last_endpoint_id: None,
                    }),
                    ..Default::default()
                },
            ),
        );
        let acknowledgement = assert_upgrade(
            read_plain(&mut &new_responder),
            bwu::EventType::ClientIntroductionAck,
        );
        assert!(acknowledgement.client_introduction_ack.is_some());

        peer.send_encrypted(&file_wire(77, b"abc"));
        peer.send_encrypted(&keepalive_wire(false, 17));
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::LastWriteToPriorChannel,
        ));
        peer.send_encrypted(&upgrade_frame(
            bwu::EventType::LastWriteToPriorChannel,
            BandwidthUpgradeNegotiationFrame::default(),
        ));
        assert_eq!(peer.receive_encrypted(), keepalive_wire(true, 17));
        let safe = assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::SafeToClosePriorChannel,
        );
        assert!(safe.safe_to_close_prior_channel.is_some());
        peer.send_encrypted(&upgrade_frame(
            bwu::EventType::SafeToClosePriorChannel,
            BandwidthUpgradeNegotiationFrame {
                safe_to_close_prior_channel: Some(
                    bwu::SafeToClosePriorChannel {
                        sta_frequency: None,
                    },
                ),
                ..Default::default()
            },
        ));
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
        .expect("complete host transition despite interleaved keepalive");
    assert_eq!(
        connection
            .receive()
            .expect("preserve interleaved file header"),
        Event::FileHeader {
            id: 77,
            total_size: 3,
            name: Some("note.txt".into()),
        }
    );
    assert_eq!(
        connection
            .receive()
            .expect("preserve interleaved file chunk"),
        Event::FileChunk {
            id: 77,
            offset: 0,
            bytes: b"abc".to_vec(),
            is_last: true,
        }
    );
    assert_eq!(
        connection
            .receive()
            .expect("preserve interleaved keepalive"),
        Event::KeepAlive {
            ack: false,
            sequence: 17,
        }
    );
    responder.join().expect("responder completes");
}

#[test]
fn upgrade_drain_rejects_excess_interleaved_frames() {
    let (old_initiator, old_responder) = UnixStream::pair().expect("old pair");
    let (new_initiator, new_responder) = UnixStream::pair().expect("new pair");
    let responder = thread::spawn(move || {
        let mut peer = RawPeer::accept(old_responder);
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::UpgradePathAvailable,
        ));
        write_plain(
            &mut &new_responder,
            &upgrade_frame(
                bwu::EventType::ClientIntroduction,
                BandwidthUpgradeNegotiationFrame::default(),
            ),
        );
        drop(assert_upgrade(
            read_plain(&mut &new_responder),
            bwu::EventType::ClientIntroductionAck,
        ));
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::LastWriteToPriorChannel,
        ));
        for id in 0..65 {
            peer.send_encrypted(&file_wire(id, b""));
        }
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
    assert!(matches!(
        connection.complete_upgrade_io(Medium::WifiLan, new_initiator),
        Err(quickshare_connections::Error::InvalidPayload)
    ));
    responder.join().expect("raw peer completes");
}

#[test]
fn upgrade_drain_stops_on_authenticated_disconnect() {
    let (old_initiator, old_responder) = UnixStream::pair().expect("old pair");
    let (new_initiator, new_responder) = UnixStream::pair().expect("new pair");
    let responder = thread::spawn(move || {
        let mut peer = RawPeer::accept(old_responder);
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::UpgradePathAvailable,
        ));
        write_plain(
            &mut &new_responder,
            &upgrade_frame(
                bwu::EventType::ClientIntroduction,
                BandwidthUpgradeNegotiationFrame::default(),
            ),
        );
        drop(assert_upgrade(
            read_plain(&mut &new_responder),
            bwu::EventType::ClientIntroductionAck,
        ));
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::LastWriteToPriorChannel,
        ));
        peer.send_encrypted(&disconnect_wire());
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
    assert!(matches!(
        connection.complete_upgrade_io(Medium::WifiLan, new_initiator),
        Err(quickshare_connections::Error::Rejected)
    ));
    responder.join().expect("raw peer completes");
}

#[test]
fn upgrade_drain_rejects_cumulative_payload_body_overflow() {
    let chunk = vec![0_u8; 600_000];
    let frames = vec![
        bytes_fragment_wire(81, 1_200_000, 0, &chunk),
        bytes_fragment_wire(81, 1_200_000, 600_000, &chunk),
    ];
    let (mut connection, new_stream, peer) =
        upgrade_host_with_raw_frames(frames);
    assert!(matches!(
        connection.complete_upgrade_io(Medium::WifiLan, new_stream),
        Err(quickshare_connections::Error::InvalidPayload)
    ));
    peer.join().expect("raw peer completes");
}

#[test]
fn upgrade_drain_preserves_authenticated_failure_event() {
    let failure = upgrade_frame(
        bwu::EventType::UpgradeFailure,
        BandwidthUpgradeNegotiationFrame {
            upgrade_path_info: Some(bwu::UpgradePathInfo {
                medium: Some(bwu::upgrade_path_info::Medium::WifiLan as i32),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let (mut connection, new_stream, peer) =
        upgrade_host_with_raw_frames(vec![failure]);
    let result = connection.complete_upgrade_io(Medium::WifiLan, new_stream);
    assert!(
        matches!(result, Err(quickshare_connections::Error::Rejected)),
        "expected authenticated failure rejection, got {result:?}"
    );
    assert_eq!(
        connection.receive().expect("preserve upgrade failure"),
        Event::Upgrade {
            event: UpgradeEvent::Failure {
                medium: Medium::WifiLan,
            },
        }
    );
    peer.join().expect("raw peer completes");
}

fn upgrade_host_with_raw_frames(
    frames: Vec<OfflineFrame>,
) -> (Connection, UnixStream, thread::JoinHandle<()>) {
    let (old_initiator, old_responder) = UnixStream::pair().expect("old pair");
    let (new_initiator, new_responder) = UnixStream::pair().expect("new pair");
    let peer = thread::spawn(move || {
        let mut peer = RawPeer::accept(old_responder);
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::UpgradePathAvailable,
        ));
        write_plain(
            &mut &new_responder,
            &upgrade_frame(
                bwu::EventType::ClientIntroduction,
                BandwidthUpgradeNegotiationFrame::default(),
            ),
        );
        drop(assert_upgrade(
            read_plain(&mut &new_responder),
            bwu::EventType::ClientIntroductionAck,
        ));
        drop(assert_upgrade(
            peer.receive_encrypted(),
            bwu::EventType::LastWriteToPriorChannel,
        ));
        for frame in frames {
            peer.send_encrypted(&frame);
        }
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
    (connection, new_initiator, peer)
}

fn assert_upgrade(
    frame: OfflineFrame,
    event: bwu::EventType,
) -> BandwidthUpgradeNegotiationFrame {
    let v1 = frame.v1.expect("V1 upgrade frame");
    assert_eq!(
        v1.r#type,
        Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32),
        "frame must use the bandwidth-upgrade envelope"
    );
    let negotiation = v1
        .bandwidth_upgrade_negotiation
        .expect("bandwidth upgrade negotiation");
    assert_eq!(
        negotiation.event_type,
        Some(event as i32),
        "upgrade event type must preserve wire ordering"
    );
    negotiation
}

fn upgrade_frame(
    event: bwu::EventType,
    body: BandwidthUpgradeNegotiationFrame,
) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32),
        bandwidth_upgrade_negotiation: Some(BandwidthUpgradeNegotiationFrame {
            event_type: Some(event as i32),
            ..body
        }),
        ..Default::default()
    })
}

fn bytes_fragment_wire(
    id: i64,
    total_size: i64,
    offset: i64,
    bytes: &[u8],
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
                total_size: Some(total_size),
                ..Default::default()
            }),
            payload_chunk: Some(payload_transfer_frame::PayloadChunk {
                offset: Some(offset),
                body: Some(bytes.to_vec()),
                flags: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

fn file_wire(id: i64, bytes: &[u8]) -> OfflineFrame {
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
                total_size: Some(
                    i64::try_from(bytes.len()).expect("file size fits i64"),
                ),
                file_name: Some("note.txt".into()),
                ..Default::default()
            }),
            payload_chunk: Some(payload_transfer_frame::PayloadChunk {
                offset: Some(0),
                body: Some(bytes.to_vec()),
                flags: Some(
                    payload_transfer_frame::payload_chunk::Flags::LastChunk
                        as i32,
                ),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

#[expect(clippy::single_call_fn, reason = "Named disconnect fixture")]
fn disconnect_wire() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::Disconnection as i32),
        disconnection: Some(DisconnectionFrame::default()),
        ..Default::default()
    })
}

fn keepalive_wire(ack: bool, sequence: u32) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::KeepAlive as i32),
        keep_alive: Some(KeepAliveFrame {
            ack: Some(ack),
            seq_num: Some(sequence),
        }),
        ..Default::default()
    })
}

fn accept_wire() -> OfflineFrame {
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

const fn offline(v1: V1Frame) -> OfflineFrame {
    OfflineFrame {
        version: Some(offline_frame::Version::V1 as i32),
        v1: Some(v1),
    }
}

fn write_plain(stream: &mut impl Write, frame: &OfflineFrame) {
    write_raw(stream, &frame.encode_to_vec());
}

fn read_plain(stream: &mut impl Read) -> OfflineFrame {
    OfflineFrame::decode(read_raw(stream).as_slice()).expect("decode frame")
}

fn write_raw(stream: &mut impl Write, bytes: &[u8]) {
    let length = u32::try_from(bytes.len()).expect("frame length fits u32");
    stream
        .write_all(&length.to_be_bytes())
        .expect("write frame length");
    stream.write_all(bytes).expect("write frame body");
    stream.flush().expect("flush frame");
}

fn read_raw(stream: &mut impl Read) -> Vec<u8> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).expect("read frame length");
    let size = usize::try_from(u32::from_be_bytes(length))
        .expect("frame length fits usize");
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).expect("read frame body");
    bytes
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
