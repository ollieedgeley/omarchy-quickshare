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
use core::time::Duration;

use base64 as _;
use prost as _;
use quickshare_connections::{Connection, ConnectionOptions, Event};
use quickshare_crypto::Handshake;
use quickshare_sharing::{ProtocolError, SharingSession};
use quickshare_wire::sharing::{
    ConnectionResponseFrame, FileMetadata, Frame, IntroductionFrame,
    PairedKeyEncryptionFrame, V1Frame, connection_response_frame,
    file_metadata, v1_frame,
};
use rand_core as _;
use serde as _;
use std::{
    io::{self, Cursor, Write},
    os::unix::net::UnixStream,
    sync::{Mutex, PoisonError, mpsc},
    thread::{self, JoinHandle},
};

/// Verifies that payload diagnostics distinguish routing from rejection.
macro_rules! assert_payload_diagnostics {
    ($diagnostics:expr, $private_sentinel:expr) => {{
        let diagnostics = $diagnostics;
        let skipped = diagnostics
            .lines()
            .find(|line| {
                line.contains("stage=\"control\"")
                    && line.contains("operation=\"demux\"")
            })
            .expect("control demux diagnostic");
        assert!(
            skipped.contains("outcome=\"skipped\""),
            "missing skipped outcome: {skipped}",
        );
        assert!(
            skipped.contains("event_type=\"response\""),
            "missing response event: {skipped}",
        );
        let rejected = diagnostics
            .lines()
            .find(|line| {
                line.contains("stage=\"validation\"")
                    && line.contains("operation=\"payload\"")
            })
            .expect("payload validation diagnostic");
        assert!(
            rejected.contains("outcome=\"rejected\""),
            "missing rejected outcome: {rejected}",
        );
        assert!(
            rejected.contains("reason=\"id_mismatch\""),
            "missing mismatch reason: {rejected}",
        );
        assert!(
            rejected.contains("id_matches_expected=false"),
            "missing mismatch flag: {rejected}",
        );
        assert!(
            !diagnostics.contains($private_sentinel),
            "diagnostics leaked the private filename: {diagnostics}",
        );
    }};
}
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

#[derive(Clone, Default)]
struct LogOutput(Arc<Mutex<Vec<u8>>>);

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }
}

impl LogOutput {
    fn contents(&self) -> String {
        String::from_utf8(
            self.0.lock().expect("lock diagnostic output").clone(),
        )
        .expect("UTF-8 diagnostics")
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

fn spawn_raw_peer<T>(
    responder_stream: UnixStream,
    peer: impl FnOnce(Connection) -> T + Send + 'static,
) -> JoinHandle<T>
where
    T: Send + 'static,
{
    thread::spawn(move || {
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
    })
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
    send_named_file_introduction(connection, "note.txt");
}

fn send_named_file_introduction(connection: &mut Connection, name: &str) {
    let introduction = Frame {
        version: Some(1_i32),
        v1: Some(V1Frame {
            r#type: Some(i32::from(v1_frame::FrameType::Introduction)),
            introduction: Some(IntroductionFrame {
                file_metadata: vec![FileMetadata {
                    id: Some(4),
                    name: Some(String::from(name)),
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
fn unnegotiated_file_completes_while_receiver_connection_remains_open() {
    let (release_sender, release_receiver) = mpsc::channel();
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let receiver = spawn_raw_peer(responder_stream, move |mut connection| {
        let _introduction = receive_bytes(&mut connection, "introduction");
        connection
            .send_sharing_frame(4, &accept_response())
            .expect("accept offer");
        let _header = connection.receive().expect("file header");
        let _data = connection.receive().expect("file data");
        assert_eq!(
            connection.receive().expect("terminal chunk"),
            Event::FileChunk {
                id: 3,
                offset: 1,
                bytes: Vec::new(),
                is_last: true,
            }
        );
        release_receiver.recv().expect("release peer");
    });
    let mut session = connect_initiator(initiator_stream);
    let (completion_sender, completion_receiver) = mpsc::channel();
    let sender = thread::spawn(move || {
        let result = session.send_outgoing_file(
            "note.txt",
            1,
            &mut Cursor::new([1_u8]),
            || {},
            |_| {},
            || false,
        );
        completion_sender
            .send(result)
            .expect("report sender completion");
    });
    let completion = completion_receiver.recv_timeout(Duration::from_secs(1));
    release_sender.send(()).expect("release peer");
    sender.join().expect("sender completes");
    receiver.join().expect("receiver completes");
    completion
        .expect("sender completes before peer disconnects")
        .expect("complete unnegotiated file");
}

#[test]
fn file_header_ignores_an_unrelated_sharing_control() {
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let sender = spawn_raw_peer(responder_stream, |mut connection| {
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
    let mut session = connect_initiator(initiator_stream);
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
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let sender = spawn_raw_peer(responder_stream, |mut connection| {
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
    let mut session = connect_initiator(initiator_stream);
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
fn diagnostics_distinguish_skipped_control_from_wrong_payload_id() {
    const PRIVATE_SENTINEL: &str = "private-sentinel.txt";

    let (release_sender, release_receiver) = mpsc::channel();
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let sender = spawn_raw_peer(responder_stream, move |mut connection| {
        send_named_file_introduction(&mut connection, PRIVATE_SENTINEL);
        connection
            .send_sharing_frame(5, &accept_response())
            .expect("send unrelated control");
        connection
            .send_file_header(9, 1, Some(String::from(PRIVATE_SENTINEL)))
            .expect("send wrong payload identifier");
        connection
            .send_file_chunk(9, 0, &[1], false)
            .expect("send wrong payload data");
        release_receiver.recv().expect("release peer");
    });
    let mut session = connect_initiator(initiator_stream);
    let offer = session.receive_incoming_offer().expect("receive offer");
    session.accept_incoming_offer().expect("accept offer");
    let output = LogOutput::default();
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "omarchy_quickshare::protocol=debug",
        ))
        .without_time()
        .with_target(true)
        .with_writer(move || LogWriter(Arc::clone(&writer.0)))
        .finish();

    let error = tracing::subscriber::with_default(subscriber, || {
        session
            .receive_incoming_file(&offer, &mut Vec::new(), |_| {}, || false)
            .expect_err("wrong payload identifier")
    });

    assert!(
        matches!(error, ProtocolError::InvalidPayload),
        "unexpected error kind: {error}",
    );
    let diagnostics = output.contents();
    assert_payload_diagnostics!(diagnostics.as_str(), PRIVATE_SENTINEL);
    release_sender.send(()).expect("release peer");
    sender.join().expect("sender completes");
}

#[test]
fn post_transfer_drain_keeps_url_connection_alive_for_peer_control() {
    let (initiator_stream, responder_stream) = bounded_stream_pair();
    let peer = spawn_raw_peer(responder_stream, |mut connection| {
        let introduction = receive_bytes(&mut connection, "URL introduction");
        let offer = SharingSession::decode_offer(&introduction)
            .expect("decode URL offer");
        assert_eq!(offer.kind(), quickshare_sharing::OfferKind::Url);
        assert_eq!(offer.size_bytes(), 21);
        connection
            .send_sharing_frame(4, &accept_response())
            .expect("accept URL");
        assert_eq!(
            receive_bytes(&mut connection, "URL payload"),
            b"https://omarchy.local"
        );
        connection.send_keepalive(42).expect("send late keepalive");
        assert_eq!(
            connection
                .receive()
                .expect("receive keepalive acknowledgement"),
            Event::KeepAlive {
                ack: true,
                sequence: 42,
            }
        );
        connection.disconnect().expect("disconnect peer");
    });
    let mut session = connect_initiator(initiator_stream);

    session
        .send_outgoing_url("https://omarchy.local", || {}, |_| {}, || false)
        .expect("write URL locally");
    session
        .drain_post_transfer_control(Duration::from_secs(1), || false)
        .expect("drain late peer control");
    peer.join().expect("peer completes");
}
