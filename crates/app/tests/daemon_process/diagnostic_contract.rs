//! Privacy-safe diagnostic contracts through the public protocol seam.

#![expect(
    clippy::expect_used,
    reason = "Diagnostic contracts keep loopback failures and fields visible"
)]

extern crate alloc;

use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use quickshare_connections::{Connection, ConnectionOptions, Error, Event};
use quickshare_crypto::Handshake;
use std::{
    env::temp_dir,
    fs::{File, remove_file},
    io::{
        ErrorKind, Read as _, Seek as _, SeekFrom, Write as _, read_to_string,
    },
    os::unix::net::UnixStream,
    process,
    sync::Mutex,
    thread,
};
use tracing::{Subscriber, subscriber::with_default};
use tracing_subscriber::EnvFilter;

const DEBUG_TARGET: &str = "DEBUG omarchy_quickshare::protocol:";
const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const PRIVATE_SENTINEL: &str = "private-peer-and-content-sentinel";
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];
const TRACE_TARGET: &str = "TRACE omarchy_quickshare::protocol:";
const UNSUPPORTED_FRAME_COUNT: usize = 4;

static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);

fn capture_file() -> File {
    let sequence = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
    let path = temp_dir().join(format!(
        "omarchy-quickshare-diagnostics-{}-{sequence}",
        process::id()
    ));
    let file = File::options()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .expect("create diagnostic capture");
    remove_file(path).expect("unlink diagnostic capture");
    file
}

fn subscriber(file: File, directive: &str) -> impl Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(directive))
        .with_ansi(false)
        .without_time()
        .with_target(true)
        .with_writer(Mutex::new(file))
        .finish()
}

fn read_capture(mut file: File) -> String {
    _ = file
        .seek(SeekFrom::Start(0))
        .expect("rewind diagnostic capture");
    read_to_string(file).expect("read diagnostic capture")
}

fn end_peer(
    stream: UnixStream,
    mut raw_stream: UnixStream,
    tail: &[u8],
    explicit_disconnect: bool,
    capture: File,
) {
    with_default(subscriber(capture, "omarchy_quickshare=debug"), || {
        let mut connection = Connection::connect_io(
            stream,
            Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
            ConnectionOptions::new(PRIVATE_SENTINEL, PRIVATE_SENTINEL)
                .with_endpoint_info(PRIVATE_SENTINEL.as_bytes().to_vec()),
        )
        .expect("establish sender encryption");
        if explicit_disconnect {
            connection.disconnect().expect("send explicit disconnect");
        } else {
            raw_stream.write_all(tail).expect("write raw tail");
            raw_stream.flush().expect("flush raw tail");
        }
    });
}

fn receive_after_peer_ends(
    tail: &[u8],
    explicit_disconnect: bool,
) -> (Result<Event, Error>, String) {
    let sender_capture = capture_file();
    let receiver_capture = capture_file();
    let sender_reader =
        sender_capture.try_clone().expect("clone sender capture");
    let receiver_reader = receiver_capture
        .try_clone()
        .expect("clone receiver capture");
    let (receiver_stream, sender_stream) =
        UnixStream::pair().expect("loopback");
    let raw_sender = sender_stream.try_clone().expect("clone sender");
    let copied_tail = tail.to_vec();
    let sender = thread::spawn(move || {
        end_peer(
            sender_stream,
            raw_sender,
            &copied_tail,
            explicit_disconnect,
            sender_capture,
        );
    });
    let result = with_default(
        subscriber(receiver_capture, "omarchy_quickshare=debug"),
        || {
            let mut connection = Connection::accept_io(
                receiver_stream,
                Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
                ConnectionOptions::new(PRIVATE_SENTINEL, PRIVATE_SENTINEL),
            )
            .expect("establish receiver encryption");
            connection.receive()
        },
    );
    sender.join().expect("sender completes");
    (
        result,
        format!(
            "{}{}",
            read_capture(sender_reader),
            read_capture(receiver_reader)
        ),
    )
}

fn assert_event(observations: &str, fields: &[(&str, &str)]) {
    assert!(
        observations.lines().any(|line| {
            line.starts_with(DEBUG_TARGET)
                && fields.iter().all(|(field, value)| {
                    line.contains(&format!("{field}=\"{value}\""))
                })
        }),
        "missing diagnostic event with {fields:?}: {observations}"
    );
}

fn assert_private_diagnostics(observations: &str) {
    assert!(
        !observations.contains(PRIVATE_SENTINEL),
        "diagnostics leaked private data: {observations}"
    );
}

fn assert_disconnected_case(fields: &[(&str, &str)], explicit: bool) {
    let (result, observations) = receive_after_peer_ends(&[], explicit);
    assert_eq!(
        result.expect("disconnect remains a protocol event"),
        Event::Disconnected,
        "wrong disconnect event"
    );
    assert_event(&observations, fields);
    assert_private_diagnostics(&observations);
}

fn assert_truncated_case(tail: &[u8], reason: &str) {
    let (result, observations) = receive_after_peer_ends(tail, false);
    let invalid_data = match &result {
        Err(Error::Io(error)) => error.kind() == ErrorKind::InvalidData,
        Ok(_) | Err(_) => false,
    };
    assert!(invalid_data, "truncated frame was not rejected: {result:?}");
    assert_event(
        &observations,
        &[
            ("stage", "framing"),
            ("operation", "read"),
            ("outcome", "rejected"),
            ("reason", reason),
            ("disconnect_origin", "stream_eof"),
            ("io_error_kind", "unexpected_eof"),
        ],
    );
    assert_private_diagnostics(&observations);
}

#[test]
fn connection_diagnostics_distinguish_disconnect_origins_and_truncation() {
    assert_disconnected_case(
        &[
            ("stage", "framing"),
            ("operation", "read"),
            ("outcome", "disconnected"),
            ("reason", "clean_eof"),
            ("disconnect_origin", "stream_eof"),
            ("io_error_kind", "unexpected_eof"),
        ],
        false,
    );
    assert_disconnected_case(
        &[
            ("stage", "control"),
            ("operation", "receive"),
            ("outcome", "disconnected"),
            ("reason", "disconnect_frame"),
            ("frame_type", "disconnection"),
            ("disconnect_origin", "explicit_frame"),
        ],
        true,
    );
    assert_truncated_case(&[0, 0], "truncated_prefix");
    assert_truncated_case(&[0, 0, 0, 4, 0], "truncated_body");
}

fn send_private_frames(stream: UnixStream, capture: File) {
    with_default(subscriber(capture, "omarchy_quickshare=trace"), || {
        let mut connection = Connection::connect_io(
            stream,
            Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
            ConnectionOptions::new(PRIVATE_SENTINEL, PRIVATE_SENTINEL)
                .with_endpoint_info(PRIVATE_SENTINEL.as_bytes().to_vec()),
        )
        .expect("establish sender encryption");
        connection
            .send_bytes(7, PRIVATE_SENTINEL.as_bytes())
            .expect("send private bytes");
        connection
            .send_file_header(
                9,
                i64::try_from(PRIVATE_SENTINEL.len()).expect("sentinel length"),
                Some(PRIVATE_SENTINEL.into()),
            )
            .expect("send private file header");
        connection
            .send_file_chunk(9, 0, PRIVATE_SENTINEL.as_bytes(), true)
            .expect("send private file chunk");
    });
}

fn receive_private_frames(stream: UnixStream, capture: File) {
    with_default(subscriber(capture, "omarchy_quickshare=trace"), || {
        let mut connection = Connection::accept_io(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new(PRIVATE_SENTINEL, PRIVATE_SENTINEL),
        )
        .expect("establish receiver encryption");
        assert_eq!(
            connection.receive().expect("receive private bytes"),
            Event::Bytes {
                id: 7,
                bytes: PRIVATE_SENTINEL.as_bytes().to_vec(),
            },
            "wrong bytes event"
        );
        assert_eq!(
            connection.receive().expect("receive private file header"),
            Event::FileHeader {
                id: 9,
                total_size: i64::try_from(PRIVATE_SENTINEL.len())
                    .expect("sentinel length"),
                name: Some(PRIVATE_SENTINEL.into()),
            },
            "wrong file header event"
        );
        assert_eq!(
            connection.receive().expect("receive private file chunk"),
            Event::FileChunk {
                id: 9,
                offset: 0,
                bytes: PRIVATE_SENTINEL.as_bytes().to_vec(),
                is_last: true,
            },
            "wrong file chunk event"
        );
    });
}

fn trace_observations() -> String {
    let sender_capture = capture_file();
    let receiver_capture = capture_file();
    let sender_reader =
        sender_capture.try_clone().expect("clone sender capture");
    let receiver_reader = receiver_capture
        .try_clone()
        .expect("clone receiver capture");
    let (receiver_stream, sender_stream) =
        UnixStream::pair().expect("loopback");
    let sender = thread::spawn(move || {
        send_private_frames(sender_stream, sender_capture);
    });
    receive_private_frames(receiver_stream, receiver_capture);
    sender.join().expect("sender completes");
    format!(
        "{}{}",
        read_capture(sender_reader),
        read_capture(receiver_reader)
    )
}

fn assert_trace_privacy(observations: &str, forbidden_fields: &[&str]) {
    assert!(
        observations
            .lines()
            .any(|line| line.starts_with(TRACE_TARGET)),
        "missing TRACE protocol diagnostic: {observations}"
    );
    assert_private_diagnostics(observations);
    for field in forbidden_fields {
        assert!(
            !observations.contains(&format!(" {field}=")),
            "diagnostics emitted forbidden field {field}: {observations}"
        );
    }
}

#[test]
fn trace_diagnostics_hide_peer_identity_and_transfer_content() {
    const FORBIDDEN_FIELDS: &[&str] = &[
        "bytes",
        "code",
        "endpoint_id",
        "endpoint_name",
        "filename",
        "frame",
        "key",
        "name",
        "path",
        "payload",
        "payload_bytes",
        "peer_address",
        "peer_id",
        "peer_name",
        "raw_frame",
        "text",
        "url",
        "verification_code",
    ];
    assert_trace_privacy(&trace_observations(), FORBIDDEN_FIELDS);
}

#[expect(
    clippy::big_endian_bytes,
    reason = "Nearby framing uses network-byte-order length prefixes"
)]
fn write_framed(stream: &mut UnixStream, bytes: &[u8]) {
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

#[expect(
    clippy::big_endian_bytes,
    reason = "Nearby framing uses network-byte-order length prefixes"
)]
fn read_framed(stream: &mut UnixStream) -> Vec<u8> {
    let mut length = [0; 4];
    stream.read_exact(&mut length).expect("read frame length");
    let mut bytes =
        vec![0; usize::try_from(u32::from_be_bytes(length)).expect("length")];
    stream.read_exact(&mut bytes).expect("read frame body");
    bytes
}

fn send_unsupported_frames(mut stream: UnixStream) {
    const ACCEPT: &[u8] =
        &[0x08, 0x01, 0x12, 0x06, 0x08, 0x02, 0x1a, 0x02, 0x18, 0x01];
    const REQUEST: &[u8] = &[0x08, 0x01, 0x12, 0x04, 0x08, 0x01, 0x12, 0x00];
    const FRAMES: &[&[u8]] = &[
        &[0x08, 0x01, 0x12, 0x02, 0x08, 0x08],
        &[0x08, 0x01, 0x12, 0x02, 0x08, 0x63],
        &[0x08, 0x01, 0x12, 0x00],
        &[0x08, 0x01, 0x12, 0x02, 0x08, 0x00],
    ];

    write_framed(&mut stream, REQUEST);
    let mut handshake =
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET);
    write_framed(
        &mut stream,
        &handshake.next_message().expect("create UKEY2 M1"),
    );
    handshake
        .receive(&read_framed(&mut stream))
        .expect("receive UKEY2 M2");
    write_framed(
        &mut stream,
        &handshake.next_message().expect("create UKEY2 M3"),
    );
    drop(read_framed(&mut stream));
    write_framed(&mut stream, ACCEPT);
    let mut channel =
        handshake.complete().expect("complete UKEY2").into_channel();
    for (nonce, frame) in (1_u8..).zip(FRAMES) {
        let encrypted = channel
            .encrypt(frame, [nonce; 16])
            .expect("encrypt unsupported frame");
        write_framed(&mut stream, &encrypted);
    }
}

fn assert_dispatch_frame(
    observations: &str,
    present: bool,
    code: Option<i32>,
    name: &str,
) {
    let line = observations
        .lines()
        .find(|line| {
            line.starts_with(DEBUG_TARGET)
                && line.contains("stage=\"frame_dispatch\"")
                && line.contains("reason=\"unexpected_frame_type\"")
                && line.contains(&format!("frame_type_present={present}"))
                && line.contains(&format!("frame_type=\"{name}\""))
                && code.is_none_or(|value| {
                    line.contains(&format!("frame_type_code={value}"))
                })
        })
        .expect("missing frame dispatch diagnostic");
    if code.is_none() {
        assert!(
            !line.contains("frame_type_code="),
            "absent discriminator gained a numeric sentinel"
        );
    }
}

#[test]
fn rejected_encrypted_frames_report_received_discriminators() {
    let capture = capture_file();
    let reader = capture.try_clone().expect("clone capture");
    let (receiver_stream, sender_stream) =
        UnixStream::pair().expect("loopback");
    let sender = thread::spawn(move || {
        send_unsupported_frames(sender_stream);
    });
    with_default(subscriber(capture, "omarchy_quickshare=debug"), || {
        let mut connection = Connection::accept_io(
            receiver_stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new(PRIVATE_SENTINEL, PRIVATE_SENTINEL),
        )
        .expect("establish receiver encryption");
        for () in [(); UNSUPPORTED_FRAME_COUNT] {
            assert!(
                matches!(connection.receive(), Err(Error::UnexpectedFrame)),
                "unsupported frame changed its public error"
            );
        }
    });
    sender.join().expect("sender completes");

    let observations = read_capture(reader);
    assert_dispatch_frame(
        &observations,
        true,
        Some(8_i32),
        "AUTHENTICATION_MESSAGE",
    );
    assert_dispatch_frame(&observations, true, Some(99_i32), "unrecognized");
    assert_dispatch_frame(&observations, false, None, "missing");
    assert_dispatch_frame(
        &observations,
        true,
        Some(0_i32),
        "UNKNOWN_FRAME_TYPE",
    );
    assert_private_diagnostics(&observations);
}
