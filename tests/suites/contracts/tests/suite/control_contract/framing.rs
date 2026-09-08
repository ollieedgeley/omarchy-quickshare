#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::pattern_type_mismatch,
    reason = "Socket contract tests assert failures and borrowed replies while propagating fixture IO errors"
)]

use core::time::Duration;
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::net::UnixStream;
use std::thread;
use std::time::Instant;

use quickshare_control::{
    MAX_MESSAGE_BYTES, PROTOCOL_VERSION, client, codec, request, response,
};
use quickshare_sharing::DiscoveryState;

use super::tests::{assert_error_code, raw_reply, with_running_endpoint};

const STATUS: &[u8] =
    include_bytes!("../../../../../fixtures/control/v7/status-request.jsonl");
const READY: &str = include_str!(
    "../../../../../fixtures/control/v7/status-ready-response.jsonl"
);

#[test]
fn silent_client_times_out_and_releases_capacity() -> io::Result<()> {
    with_running_endpoint(|socket| {
        let start = Instant::now();
        assert_error_code(&raw_reply(socket, b"")?, "request_timeout")?;
        assert!(start.elapsed() < Duration::from_secs(6));
        assert_eq!(raw_reply(socket, STATUS)?, READY);
        Ok(())
    })
}

#[test]
fn excess_connections_are_closed_without_accepting_work() -> io::Result<()> {
    with_running_endpoint(|socket| {
        let mut pending = Vec::new();
        for _client in 0_u8..32 {
            let mut stream = UnixStream::connect(socket)?;
            stream.write_all(b" ")?;
            pending.push(stream);
        }
        let mut excess = UnixStream::connect(socket)?;
        excess.set_read_timeout(Some(Duration::from_secs(2)))?;
        // A rejected stream may close before its bytes can be written.
        let _written = excess.write_all(
            format!("{{\"request\":{{\"type\":\"discover\"}},\"version\":{PROTOCOL_VERSION}}}\n").as_bytes(),
        );
        let mut byte = [0];
        match excess.read(&mut byte) {
            Ok(0) => {}
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => {}
            result => panic!("excess client was not rejected: {result:?}"),
        }
        // Free admitted clients through their normal framing path.
        for mut stream in pending {
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.write_all(STATUS)?;
            let mut reply = String::new();
            let _read = BufReader::new(stream).read_line(&mut reply)?;
            assert_eq!(reply, READY);
        }
        let observed =
            client::exchange(socket, &request::Envelope::snapshot())?;
        let response::Response::Snapshot { snapshot, .. } = observed.response()
        else {
            panic!("expected snapshot: {observed:?}");
        };
        assert_eq!(snapshot.discovery(), DiscoveryState::Idle);
        Ok(())
    })
}

#[test]
fn ordinary_connection_ignores_a_coalesced_second_command() -> io::Result<()> {
    with_running_endpoint(|socket| {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(format!("{{\"request\":{{\"type\":\"status\"}},\"version\":{PROTOCOL_VERSION}}}\n{{\"request\":{{\"type\":\"discover\"}},\"version\":{PROTOCOL_VERSION}}}\n").as_bytes())?;
        let mut replies = String::new();
        let _read = stream.read_to_string(&mut replies)?;
        assert_eq!(replies, READY);
        let observed =
            client::exchange(socket, &request::Envelope::snapshot())?;
        let response::Response::Snapshot { snapshot, .. } = observed.response()
        else {
            panic!("expected snapshot: {observed:?}");
        };
        assert_eq!(snapshot.discovery(), DiscoveryState::Idle);
        Ok(())
    })
}

#[test]
fn client_rejects_oversized_truncated_and_wrong_version_replies()
-> io::Result<()> {
    let oversized = vec![b' '; MAX_MESSAGE_BYTES + 1];
    for (reply, kind) in [
        (oversized, io::ErrorKind::FileTooLarge),
        (
            READY.trim_end().as_bytes().to_vec(),
            io::ErrorKind::UnexpectedEof,
        ),
        (
            b"{\"response\":{\"type\":\"ready\"},\"version\":255}\n".to_vec(),
            io::ErrorKind::InvalidData,
        ),
    ] {
        let (stream, mut peer) = UnixStream::pair()?;
        let worker = thread::spawn(move || -> io::Result<()> {
            let mut request = String::new();
            let _read =
                BufReader::new(peer.try_clone()?).read_line(&mut request)?;
            let _written = peer.write_all(&reply);
            Ok(())
        });
        let result =
            client::exchange_stream(stream, &request::Envelope::status());
        worker
            .join()
            .map_err(|_panic| io::Error::other("peer panicked"))??;
        assert_eq!(result.expect_err("invalid reply accepted").kind(), kind);
    }
    Ok(())
}

#[test]
#[expect(
    clippy::integer_division,
    clippy::integer_division_remainder_used,
    reason = "Each fixture character occupies two UTF-8 bytes; envelope overhead exceeds the frame limit"
)]
fn oversized_outgoing_response_writes_no_partial_frame() -> io::Result<()> {
    let (mut stream, mut peer) = UnixStream::pair()?;
    peer.set_read_timeout(Some(Duration::from_secs(2)))?;
    let oversized =
        response::Envelope::preferences(response::PreferenceStatus {
            saved: Some(response::PreferenceValues {
                device_name: Some("\u{e9}".repeat(MAX_MESSAGE_BYTES / 2)),
                ..response::PreferenceValues::default()
            }),
            ..response::PreferenceStatus::default()
        });
    let result = codec::write_response(&mut stream, &oversized);
    drop(stream);
    let mut received = Vec::new();
    let _read = peer.read_to_end(&mut received)?;
    assert_eq!(
        result.expect_err("oversized response accepted").kind(),
        io::ErrorKind::FileTooLarge
    );
    assert!(
        received.is_empty(),
        "oversized response leaked a partial frame"
    );
    Ok(())
}
