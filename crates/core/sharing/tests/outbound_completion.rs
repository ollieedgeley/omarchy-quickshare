//! Outbound Sharing completion contracts.

#![expect(
    clippy::absolute_paths,
    clippy::expect_used,
    clippy::missing_trait_methods,
    clippy::tests_outside_test_module,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

extern crate alloc;

use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use base64 as _;
use prost as _;
use quickshare_connections::{Connection, ConnectionOptions, Event};
use quickshare_crypto::Handshake;
use quickshare_sharing::{OfferKind, ProtocolError, SharingSession};
use quickshare_wire::sharing::{
    ConnectionResponseFrame, FileMetadata, Frame, IntroductionFrame,
    PairedKeyEncryptionFrame, V1Frame, connection_response_frame,
    file_metadata, v1_frame,
};
use rand_core as _;
use serde as _;
use std::{
    io::{self, Cursor, Read, Write},
    os::unix::net::UnixStream,
    sync::mpsc::{self, Sender, sync_channel},
    thread::{self, JoinHandle},
};

const IO_TIMEOUT: Duration = Duration::from_secs(5);
const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

struct NotifyingIo {
    deadline_set: Sender<()>,
    inner: UnixStream,
}

impl Read for NotifyingIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl Write for NotifyingIo {
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
}

impl quickshare_connections::ConnectionIo for NotifyingIo {
    fn set_read_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.inner.set_read_timeout(Some(timeout))?;
        let _sent = self.deadline_set.send(());
        Ok(())
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.inner.shutdown(std::net::Shutdown::Write)
    }
}

struct TimeoutIo {
    inner: UnixStream,
    read_timeout: bool,
}

impl Read for TimeoutIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.read_timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "completion deadline",
            ));
        }
        self.inner.read(buf)
    }
}

impl Write for TimeoutIo {
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
}

impl quickshare_connections::ConnectionIo for TimeoutIo {
    fn set_read_timeout(&mut self, _timeout: Duration) -> io::Result<()> {
        self.read_timeout = true;
        Ok(())
    }

    fn shutdown_write(&mut self) -> io::Result<()> {
        self.inner.shutdown(std::net::Shutdown::Write)
    }
}

fn bounded_stream_pair() -> (UnixStream, UnixStream) {
    let (initiator_stream, responder_stream) =
        UnixStream::pair().expect("unix pair");
    for stream in [&initiator_stream, &responder_stream] {
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .expect("bound reads");
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .expect("bound writes");
    }
    (initiator_stream, responder_stream)
}

fn connect_initiator<Stream>(stream: Stream) -> SharingSession
where
    Stream: quickshare_connections::ConnectionIo + 'static,
{
    let connection = Connection::connect_io(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("local", "Omarchy"),
    )
    .expect("establish local session");
    let mut session = SharingSession::new(connection);
    let _pairing = session.exchange_account_free_pairing().expect("pair");
    session
}

fn paired_session<T>(
    peer: impl FnOnce(SharingSession) -> T + Send + 'static,
) -> (SharingSession, JoinHandle<T>)
where
    T: Send + 'static,
{
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let responder = thread::spawn(move || {
        let connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let mut session = SharingSession::new(connection);
        let _pairing = session.exchange_account_free_pairing().expect("pair");
        peer(session)
    });
    (connect_initiator(initiator_stream), responder)
}

fn paired_raw_peer<T>(
    peer: impl FnOnce(Connection) -> T + Send + 'static,
) -> (SharingSession, JoinHandle<T>)
where
    T: Send + 'static,
{
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    paired_raw_peer_with(initiator_stream, responder_stream, peer)
}

fn paired_raw_peer_with<Stream, T>(
    initiator_stream: Stream,
    responder_stream: UnixStream,
    peer: impl FnOnce(Connection) -> T + Send + 'static,
) -> (SharingSession, JoinHandle<T>)
where
    Stream: quickshare_connections::ConnectionIo + 'static,
    T: Send + 'static,
{
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            responder_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session");
        let _encryption = receive_bytes(&mut connection, "pairing encryption");
        let pairing_encryption = Frame {
            version: Some(1_i32),
            v1: Some(V1Frame {
                r#type: Some(i32::from(
                    v1_frame::FrameType::PairedKeyEncryption,
                )),
                paired_key_encryption: Some(PairedKeyEncryptionFrame {
                    signed_data: Some(vec![0; 72]),
                    secret_id_hash: Some(vec![0; 6]),
                    optional_signed_data: None,
                    qr_code_handshake_data: None,
                }),
                ..Default::default()
            }),
        };
        connection
            .send_sharing_frame(1, &pairing_encryption)
            .expect("send pairing encryption");
        let _result = receive_bytes(&mut connection, "pairing result");
        connection
            .send_sharing_frame(2, &SharingSession::account_free_result())
            .expect("send pairing result");
        peer(connection)
    });
    (connect_initiator(initiator_stream), responder)
}

fn receive_bytes(connection: &mut Connection, label: &str) -> Vec<u8> {
    match connection.receive().expect(label) {
        Event::Bytes { bytes, .. } => Some(bytes),
        Event::FileHeader { .. }
        | Event::FileChunk { .. }
        | Event::KeepAlive { .. }
        | Event::Upgrade { .. }
        | Event::PayloadError { .. }
        | Event::PayloadCancelled { .. }
        | Event::Disconnected
        | _ => None,
    }
    .expect(label)
}

fn accept_outgoing_file(connection: &mut Connection) {
    let _introduction = receive_bytes(connection, "introduction");
    connection
        .send_sharing_frame(4, &accept_response())
        .expect("accept offer");
    assert!(
        matches!(
            connection.receive().expect("file header"),
            Event::FileHeader { .. }
        ),
        "expected file header",
    );
    assert!(
        matches!(
            connection.receive().expect("file data"),
            Event::FileChunk { is_last: false, .. }
        ),
        "expected nonterminal file data",
    );
    assert!(
        matches!(
            connection.receive().expect("terminal chunk"),
            Event::FileChunk { is_last: true, .. }
        ),
        "expected terminal file chunk",
    );
}

fn send_one_byte_file<Cancelled>(
    session: &mut SharingSession,
    is_cancelled: Cancelled,
) -> Result<(), ProtocolError>
where
    Cancelled: Fn() -> bool,
{
    session.send_outgoing_file(
        "note.txt",
        1,
        &mut Cursor::new([1_u8]),
        || {},
        |_| {},
        is_cancelled,
    )
}

fn receive_one_byte_file(
    session: &mut SharingSession,
    expectation: &str,
) -> Vec<u8> {
    let offer = session.receive_incoming_offer().expect("receive offer");
    session.accept_incoming_offer().expect("accept offer");
    let mut received = Vec::new();
    session
        .receive_incoming_file(&offer, &mut received, |_| {}, || false)
        .expect(expectation);
    received
}

fn send_file_introduction(connection: &mut Connection) {
    let introduction = Frame {
        version: Some(1_i32),
        v1: Some(V1Frame {
            r#type: Some(i32::from(v1_frame::FrameType::Introduction)),
            introduction: Some(IntroductionFrame {
                file_metadata: vec![FileMetadata {
                    id: Some(4),
                    name: Some(String::from("note.txt")),
                    r#type: Some(i32::from(file_metadata::Type::Document)),
                    payload_id: Some(3),
                    size: Some(1),
                    ..Default::default()
                }],
                start_transfer: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }),
    };
    connection
        .send_sharing_frame(4, &introduction)
        .expect("send introduction");
    let _response = receive_bytes(connection, "accept response");
}

fn accept_response() -> Frame {
    Frame {
        version: Some(1),
        v1: Some(V1Frame {
            r#type: Some(i32::from(v1_frame::FrameType::Response)),
            connection_response: Some(ConnectionResponseFrame {
                status: Some(i32::from(
                    connection_response_frame::Status::Accept,
                )),
                ..Default::default()
            }),
            ..Default::default()
        }),
    }
}

#[test]
fn file_cancellation_after_final_bytes_prevents_completion() {
    let (mut session, receiver) = paired_session(|mut session| {
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.kind(), OfferKind::File);
        session.accept_incoming_offer().expect("accept offer");
        session
            .receive_incoming_file(&offer, &mut Vec::new(), |_| {}, || false)
            .expect("receive file");
        let error = session
            .receive_incoming_file(&offer, &mut Vec::new(), |_| {}, || true)
            .expect_err("cancel completed payload");
        assert!(matches!(error, ProtocolError::Cancelled));
    });

    let error = send_one_byte_file(&mut session, || false)
        .expect_err("receiver cancellation must prevent completion");

    assert!(matches!(error, ProtocolError::Cancelled));
    receiver.join().expect("receiver completes");
}

#[test]
fn url_cancellation_after_final_bytes_prevents_completion() {
    let (cancel_sent, cancel_received) = sync_channel(0);
    let (mut session, receiver) = paired_session(move |mut session| {
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.kind(), OfferKind::Url);
        session.accept_incoming_offer().expect("accept offer");
        assert_eq!(
            session
                .receive_incoming_url(&offer, |_| {}, || false)
                .expect("receive URL"),
            "https://omarchy.local"
        );
        let error = session
            .receive_incoming_url(&offer, |_| {}, || true)
            .expect_err("cancel completed payload");
        assert!(matches!(error, ProtocolError::Cancelled));
        cancel_sent.send(()).expect("report cancellation sent");
    });

    let error = session
        .send_outgoing_url(
            "https://omarchy.local",
            || {},
            |_| cancel_received.recv().expect("receiver cancellation"),
            || false,
        )
        .expect_err("receiver cancellation must prevent completion");

    assert!(matches!(error, ProtocolError::Cancelled));
    receiver.join().expect("receiver completes");
}

#[test]
fn file_payload_ends_with_an_empty_terminal_chunk() {
    let (mut session, receiver) = paired_raw_peer(|mut connection| {
        let introduction = receive_bytes(&mut connection, "introduction");
        let offer = SharingSession::decode_offer(&introduction)
            .expect("decode introduction");
        connection
            .send_sharing_frame(4, &accept_response())
            .expect("accept offer");
        assert_eq!(
            connection.receive().expect("file header"),
            Event::FileHeader {
                id: offer.payload_id(),
                total_size: 1,
                name: Some(String::from("note.txt")),
            }
        );
        assert_eq!(
            connection.receive().expect("file data"),
            Event::FileChunk {
                id: offer.payload_id(),
                offset: 0,
                bytes: vec![1],
                is_last: false,
            }
        );
        assert_eq!(
            connection.receive().expect("terminal chunk"),
            Event::FileChunk {
                id: offer.payload_id(),
                offset: 1,
                bytes: Vec::new(),
                is_last: true,
            }
        );
        connection.disconnect().expect("disconnect receiver");
    });

    send_one_byte_file(&mut session, || false)
        .expect("complete file after receiver disconnect");
    receiver.join().expect("receiver completes");
}

#[test]
fn file_header_ignores_an_unrelated_sharing_control() {
    let (mut session, sender) = paired_raw_peer(|mut connection| {
        send_file_introduction(&mut connection);
        connection
            .send_sharing_frame(5, &accept_response())
            .expect("send unrelated control");
        connection
            .send_file_header(3, 1, Some(String::from("note.txt")))
            .expect("declare file");
        connection
            .send_file_chunk(3, 0, &[1], true)
            .expect("send file");
    });
    assert_eq!(
        receive_one_byte_file(
            &mut session,
            "ignore control before file header"
        ),
        [1],
    );
    sender.join().expect("sender completes");
}

#[test]
fn file_chunks_ignore_an_unrelated_sharing_control() {
    let (mut session, sender) = paired_raw_peer(|mut connection| {
        send_file_introduction(&mut connection);
        connection
            .send_file_header(3, 1, Some(String::from("note.txt")))
            .expect("declare file");
        connection
            .send_file_chunk(3, 0, &[1], false)
            .expect("send file data");
        connection
            .send_sharing_frame(5, &accept_response())
            .expect("send unrelated control");
        connection
            .send_file_chunk(3, 1, &[], true)
            .expect("finish file");
    });
    assert_eq!(
        receive_one_byte_file(
            &mut session,
            "ignore control between file chunks",
        ),
        [1],
    );
    sender.join().expect("sender completes");
}

#[test]
fn outgoing_completion_ignores_an_unrelated_sharing_control() {
    let (mut session, receiver) = paired_raw_peer(|mut connection| {
        accept_outgoing_file(&mut connection);
        connection
            .send_sharing_frame(5, &accept_response())
            .expect("send unrelated control");
        connection.disconnect().expect("disconnect receiver");
    });

    send_one_byte_file(&mut session, || false)
        .expect("complete after receiver disconnect");
    receiver.join().expect("receiver completes");
}

#[test]
fn outgoing_completion_has_a_fixed_deadline() {
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let timeout_io = TimeoutIo {
        inner: initiator_stream,
        read_timeout: false,
    };
    let (release_sender, release_receiver) = mpsc::channel();
    let (mut session, receiver) = paired_raw_peer_with(
        timeout_io,
        responder_stream,
        move |mut connection| {
            accept_outgoing_file(&mut connection);
            release_receiver.recv().expect("release receiver");
        },
    );

    let error = send_one_byte_file(&mut session, || false)
        .expect_err("completion deadline");

    assert!(matches!(
        &error,
        ProtocolError::Connection(quickshare_connections::Error::Io(
            inner_error,
        )) if inner_error.kind() == io::ErrorKind::TimedOut
    ));
    release_sender.send(()).expect("release receiver");
    receiver.join().expect("receiver completes");
}

#[test]
fn receiver_disconnect_does_not_override_local_cancellation() {
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let (deadline_set, deadline_observed) = mpsc::channel();
    let notifying_io = NotifyingIo {
        inner: initiator_stream,
        deadline_set,
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let peer_cancelled = Arc::clone(&cancelled);
    let (mut session, receiver) = paired_raw_peer_with(
        notifying_io,
        responder_stream,
        move |mut connection| {
            accept_outgoing_file(&mut connection);
            deadline_observed.recv().expect("completion wait started");
            peer_cancelled.store(true, Ordering::Release);
            connection.disconnect().expect("disconnect receiver");
        },
    );

    let error =
        send_one_byte_file(&mut session, || cancelled.load(Ordering::Acquire))
            .expect_err("local cancellation wins");

    assert!(matches!(error, ProtocolError::Cancelled));
    receiver.join().expect("receiver completes");
}
