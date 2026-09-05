//! Absolute-deadline and cancellation behavior at partial frame boundaries.

#![expect(
    clippy::as_conversions,
    clippy::big_endian_bytes,
    clippy::expect_used,
    clippy::missing_trait_methods,
    clippy::tests_outside_test_module,
    reason = "The fake stream and manual peer expose exact framing behavior"
)]

extern crate alloc;

use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use prost::Message as _;
use quickshare_connections::{
    Connection, ConnectionIo, ConnectionOptions, Error, Event,
};
use quickshare_crypto::{Handshake, SecureChannel};
use quickshare_wire::connections::{
    BandwidthUpgradeRetryFrame, ConnectionRequestFrame,
    ConnectionResponseFrame, KeepAliveFrame, OfflineFrame,
    PayloadTransferFrame, V1Frame, bandwidth_upgrade_retry_frame,
    connection_response_frame, offline_frame, payload_transfer_frame, v1_frame,
};
use rand_core as _;
use rustix as _;
use std::{
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::mpsc,
    thread,
};
use tracing as _;

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

struct ResetAfterWriteIo {
    enabled: Arc<AtomicBool>,
    reset: bool,
    stream: UnixStream,
}

struct TimeoutIo {
    cancel: Option<Arc<AtomicBool>>,
    enabled: Arc<AtomicBool>,
    reads: u8,
    stream: UnixStream,
    writes: u8,
}

impl Read for ResetAfterWriteIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.enabled.load(Ordering::Acquire) && self.reset {
            return Err(io::Error::from(io::ErrorKind::ConnectionReset));
        }
        self.stream.read(buf)
    }
}

impl Write for ResetAfterWriteIo {
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.stream.write(buf)?;
        if self.enabled.load(Ordering::Acquire) {
            self.reset = true;
        }
        Ok(written)
    }
}

impl ConnectionIo for ResetAfterWriteIo {
    fn read_ready(&self) -> io::Result<bool> {
        self.stream.read_ready()
    }
    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        self.stream.read_timeout()
    }
    fn set_read_timeout(
        &mut self,
        timeout: Option<Duration>,
    ) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }
    fn set_write_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.stream.set_write_timeout(Some(timeout))
    }
    fn shutdown_write(&mut self) -> io::Result<()> {
        self.stream.shutdown(Shutdown::Write)
    }
}

impl TimeoutIo {
    const fn new(
        stream: UnixStream,
        enabled: Arc<AtomicBool>,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Self {
        Self {
            cancel,
            enabled,
            reads: 0,
            stream,
            writes: 0,
        }
    }
}

impl Read for TimeoutIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.enabled.load(Ordering::Acquire) {
            return self.stream.read(buf);
        }
        self.reads = self.reads.saturating_add(1);
        if self.reads == 1 {
            let limit = buf.len().min(1);
            let prefix = buf
                .get_mut(..limit)
                .ok_or_else(|| io::Error::other("invalid read prefix"))?;
            return self.stream.read(prefix);
        }
        if self.reads == 2 {
            if let Some(cancel) = self.cancel.as_ref() {
                cancel.store(true, Ordering::Release);
            }
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        self.stream.read(buf)
    }
}

impl Write for TimeoutIo {
    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.enabled.load(Ordering::Acquire) || self.cancel.is_some() {
            return self.stream.write(buf);
        }
        self.writes = self.writes.saturating_add(1);
        if self.writes == 1 {
            let prefix = buf
                .get(..buf.len().min(1))
                .ok_or_else(|| io::Error::other("invalid write prefix"))?;
            return self.stream.write(prefix);
        }
        if self.writes == 2 {
            return Err(io::Error::from(io::ErrorKind::WouldBlock));
        }
        self.stream.write(buf)
    }
}

impl ConnectionIo for TimeoutIo {
    fn read_ready(&self) -> io::Result<bool> {
        self.stream.read_ready()
    }
    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        self.stream.read_timeout()
    }
    fn set_read_timeout(
        &mut self,
        timeout: Option<Duration>,
    ) -> io::Result<()> {
        self.stream.set_read_timeout(timeout)
    }
    fn set_write_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.stream.set_write_timeout(Some(timeout))
    }
    fn shutdown_write(&mut self) -> io::Result<()> {
        self.stream.shutdown(Shutdown::Write)
    }
}

#[test]
fn drain_preserves_partial_prefixes_across_poll_timeouts() {
    let (local, mut peer) = UnixStream::pair().expect("unix pair");
    let enabled = Arc::new(AtomicBool::new(false));
    let peer_thread = thread::spawn(move || {
        let mut channel = initiate(&mut peer);
        send_encrypted(&mut peer, &mut channel, &retry(), [7; 16]);
        send_encrypted(&mut peer, &mut channel, &keepalive(false, 43), [8; 16]);
        let acknowledged = channel
            .decrypt(&read_frame(&mut peer))
            .expect("decrypt ACK");
        assert_eq!(
            OfflineFrame::decode(acknowledged.as_slice()).expect("decode ACK"),
            keepalive(true, 43),
            "peer must receive the matching KeepAlive acknowledgement"
        );
    });
    let connection = Connection::accept_io(
        TimeoutIo::new(local, Arc::clone(&enabled), None),
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder");
    enabled.store(true, Ordering::Release);
    connection
        .drain_post_transfer_control(Duration::from_secs(1), || false)
        .expect("drain across injected timeouts");
    peer_thread.join().expect("raw peer completes");
}

#[test]
fn drain_accepts_peer_close_racing_keepalive_ack_write() {
    let (local, mut peer) = UnixStream::pair().expect("unix pair");
    let peer_thread = thread::spawn(move || {
        let mut channel = initiate(&mut peer);
        send_encrypted(&mut peer, &mut channel, &keepalive(false, 46), [7; 16]);
        peer.shutdown(Shutdown::Both).expect("close peer");
    });
    let connection = Connection::accept_io(
        local,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder");
    connection
        .drain_post_transfer_control(Duration::from_secs(1), || false)
        .expect("peer closure completes drain");
    peer_thread.join().expect("raw peer completes");
}

#[test]
fn drain_accepts_reset_after_keepalive_ack_write() {
    let (local, mut peer) = UnixStream::pair().expect("unix pair");
    let enabled = Arc::new(AtomicBool::new(false));
    let peer_thread = thread::spawn(move || {
        let mut channel = initiate(&mut peer);
        send_encrypted(&mut peer, &mut channel, &keepalive(false, 47), [7; 16]);
        let acknowledged = channel
            .decrypt(&read_frame(&mut peer))
            .expect("decrypt ACK");
        assert_eq!(
            OfflineFrame::decode(acknowledged.as_slice()).expect("decode ACK"),
            keepalive(true, 47),
            "peer must receive the matching KeepAlive acknowledgement"
        );
    });
    let connection = Connection::accept_io(
        ResetAfterWriteIo {
            enabled: Arc::clone(&enabled),
            reset: false,
            stream: local,
        },
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder");
    enabled.store(true, Ordering::Release);
    connection
        .drain_post_transfer_control(Duration::from_secs(1), || false)
        .expect("connection reset confirms peer closure");
    peer_thread.join().expect("raw peer completes");
}

#[test]
fn drain_cancels_after_partial_prefix_before_peer_release() {
    let (local, mut peer) = UnixStream::pair().expect("unix pair");
    let enabled = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let (release_sender, release_receiver) = mpsc::channel();
    let peer_thread = thread::spawn(move || {
        let mut channel = initiate(&mut peer);
        let bytes = channel
            .encrypt(&keepalive(false, 44).encode_to_vec(), [7; 16])
            .expect("encrypt partial frame");
        let length = u32::try_from(bytes.len())
            .expect("frame length fits u32")
            .to_be_bytes();
        let prefix = length.get(..1).expect("one-byte prefix");
        peer.write_all(prefix).expect("partial prefix");
        peer.flush().expect("flush partial prefix");
        release_receiver.recv().expect("release peer");
    });
    let connection = Connection::accept_io(
        TimeoutIo::new(
            local,
            Arc::clone(&enabled),
            Some(Arc::clone(&cancelled)),
        ),
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder");
    enabled.store(true, Ordering::Release);
    let (result_sender, result_receiver) = mpsc::channel();
    let drain_thread = thread::spawn(move || {
        let result = connection
            .drain_post_transfer_control(Duration::from_secs(10), || {
                cancelled.load(Ordering::Acquire)
            });
        result_sender.send(result).expect("report drain result");
    });
    let result = result_receiver.recv_timeout(Duration::from_secs(1));
    release_sender.send(()).expect("release peer");
    drain_thread.join().expect("drain thread completes");
    peer_thread.join().expect("raw peer completes");
    assert!(matches!(result, Ok(Err(Error::Cancelled))));
}

#[test]
fn poll_event_reassembles_reference_body_then_empty_last() {
    let (local, mut peer) = UnixStream::pair().expect("unix pair");
    let (ready_sender, ready_receiver) = mpsc::channel();
    let peer_thread = thread::spawn(move || {
        let mut channel = initiate(&mut peer);
        send_encrypted(
            &mut peer,
            &mut channel,
            &bytes_chunk(51, 6, 0, b"cancel", false),
            [7; 16],
        );
        send_encrypted(
            &mut peer,
            &mut channel,
            &bytes_chunk(51, 6, 6, b"", true),
            [8; 16],
        );
        ready_sender.send(()).expect("payload written");
    });
    let mut connection = Connection::accept_io(
        local,
        Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
        ConnectionOptions::new("responder", "responder"),
    )
    .expect("establish responder");
    ready_receiver.recv().expect("payload ready");
    assert_eq!(
        connection.poll_event().expect("first poll"),
        None,
        "the body frame must remain incomplete"
    );
    assert_eq!(
        connection.poll_event().expect("second poll"),
        Some(Event::Bytes {
            id: 51,
            bytes: b"cancel".to_vec(),
        }),
        "the empty LAST frame must complete the BYTES payload"
    );
    peer_thread.join().expect("raw peer completes");
}

fn initiate(stream: &mut UnixStream) -> SecureChannel {
    write_frame(stream, &request().encode_to_vec());
    let mut handshake =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    write_frame(stream, &handshake.next_message().expect("M1"));
    handshake.receive(&read_frame(stream)).expect("M2");
    write_frame(stream, &handshake.next_message().expect("M3"));
    write_frame(stream, &accept().encode_to_vec());
    assert_eq!(
        OfflineFrame::decode(read_frame(stream).as_slice()).expect("ACCEPT"),
        accept(),
        "peer must receive the matching acceptance"
    );
    handshake.complete().expect("complete UKEY2").into_channel()
}

#[expect(clippy::single_call_fn, reason = "Named request fixture")]
fn request() -> OfflineFrame {
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
fn accept() -> OfflineFrame {
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
#[expect(clippy::single_call_fn, reason = "Named retry fixture")]
fn retry() -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::BandwidthUpgradeRetry as i32),
        bandwidth_upgrade_retry: Some(BandwidthUpgradeRetryFrame {
            supported_medium: vec![
                bandwidth_upgrade_retry_frame::Medium::WifiLan as i32,
            ],
            is_request: Some(true),
        }),
        ..Default::default()
    })
}
fn keepalive(ack: bool, seq_num: u32) -> OfflineFrame {
    offline(V1Frame {
        r#type: Some(v1_frame::FrameType::KeepAlive as i32),
        keep_alive: Some(KeepAliveFrame {
            ack: Some(ack),
            seq_num: Some(seq_num),
        }),
        ..Default::default()
    })
}
fn bytes_chunk(
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

const fn offline(v1: V1Frame) -> OfflineFrame {
    OfflineFrame {
        version: Some(offline_frame::Version::V1 as i32),
        v1: Some(v1),
    }
}
fn send_encrypted(
    stream: &mut UnixStream,
    channel: &mut SecureChannel,
    frame: &OfflineFrame,
    iv: [u8; 16],
) {
    let bytes = channel
        .encrypt(&frame.encode_to_vec(), iv)
        .expect("encrypt");
    write_frame(stream, &bytes);
}
fn write_frame(stream: &mut UnixStream, bytes: &[u8]) {
    let length = u32::try_from(bytes.len()).expect("frame length fits u32");
    stream.write_all(&length.to_be_bytes()).expect("length");
    stream.write_all(bytes).expect("body");
    stream.flush().expect("flush");
}
fn read_frame(stream: &mut UnixStream) -> Vec<u8> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).expect("length");
    let size = usize::try_from(u32::from_be_bytes(length))
        .expect("frame length fits usize");
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).expect("body");
    bytes
}
