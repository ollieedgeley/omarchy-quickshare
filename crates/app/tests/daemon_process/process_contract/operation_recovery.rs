#![expect(
    clippy::panic,
    clippy::panic_in_result_fn,
    reason = "Recovery contracts propagate fixture errors and assert operation outcomes"
)]

use core::time::Duration;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;
use std::time::Instant;

use quickshare_control::client;
use quickshare_control::codec::{read_response, write_request};
use quickshare_control::request::Envelope as Request;
use quickshare_control::response::{ErrorCode, OperationRecord, Response};
use quickshare_sharing::Phase;

use super::{
    DaemonProcessFixture, assert_command, run_command, runtime_fixture_path,
};

#[test]
fn restart_reports_interrupted_accepted_work_without_replaying_payload()
-> io::Result<()> {
    let root = runtime_fixture_path();
    let first = DaemonProcessFixture::start(root.clone())?;
    assert_command(
        first.runtime_directory(),
        &["send", "private interrupted payload"],
    );
    first.kill()?;

    let restarted = DaemonProcessFixture::start(root)?;
    let output =
        run_command(restarted.runtime_directory(), &["status", "--json"])?;
    assert!(
        output.status.success(),
        "recovered snapshot must be observable"
    );
    let envelope = read_response(&mut output.stdout.as_slice())?;
    let Response::Snapshot {
        snapshot,
        operations,
        ..
    } = envelope.response()
    else {
        panic!("expected recovered snapshot");
    };
    assert!(snapshot.active_share().is_none());
    let [operation] = operations.as_slice() else {
        panic!("expected retained accepted operation");
    };
    assert_eq!(operation.id_string, "1");
    assert_eq!(operation.phase, Phase::Failed);
    assert_eq!(operation.terminal_reason.as_deref(), Some("interrupted"));
    let snapshot_json = String::from_utf8_lossy(&output.stdout);
    assert!(!snapshot_json.contains("private interrupted payload"));
    restarted.stop()
}

#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Every response other than an operation or retryable outcome must fail this contract"
)]
fn operation(root: &Path, request_id: &str) -> io::Result<OperationRecord> {
    let started = Instant::now();
    loop {
        let output = run_command(root, &["operation", request_id, "--json"])?;
        let envelope = read_response(&mut output.stdout.as_slice())?;
        match envelope.response() {
            Response::Operation { operation } => {
                assert!(output.status.success());
                return Ok(operation.clone());
            }
            Response::Error {
                code: ErrorCode::OutcomeUnknown,
            } if started.elapsed() < Duration::from_secs(1) => {
                thread::sleep(Duration::from_millis(5));
            }
            response => panic!("operation not observed: {response:?}"),
        }
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "The recovery scenario keeps lost reply, dismissal, and restart assertions in execution order"
)]
fn lost_reply_reconciles_and_completed_dismissal_survives_restart()
-> io::Result<()> {
    let root = runtime_fixture_path();
    let fixture = DaemonProcessFixture::start_simulated(root.clone())?;
    let socket = root.join("omarchy-quickshare/control.sock");
    let mut lost_reply = UnixStream::connect(&socket)?;
    write_request(
        &mut lost_reply,
        &Request::submit_text("hello", "lost-reply"),
    )?;
    drop(lost_reply);
    let accepted = operation(&root, "lost-reply")?;
    assert_eq!(accepted.id_string, "1");
    assert_eq!(accepted.request_id.as_deref(), Some("lost-reply"));
    assert_eq!(accepted.phase, Phase::WaitingForPeer);
    for (request, expected) in [
        (
            Request::submit_text("hello", "lost-reply"),
            ErrorCode::Conflict,
        ),
        (
            Request::submit_text("other", "while-active"),
            ErrorCode::Busy,
        ),
    ] {
        let response = client::exchange(&socket, &request)?;
        assert!(matches!(
            response.response(),
            Response::Error { code } if *code == expected
        ));
    }
    assert_eq!(operation(&root, "lost-reply")?, accepted);
    assert_command(&root, &["share", "select", "1", "pixel-8"]);
    assert_command(&root, &["simulate", "peer-accept", "1"]);
    let mut unread_snapshot = UnixStream::connect(&socket)?;
    write_request(&mut unread_snapshot, &Request::snapshot())?;
    assert_command(&root, &["simulate", "progress", "1", "5"]);
    let progressing = operation(&root, "lost-reply")?;
    assert_eq!(progressing.phase, Phase::Transferring);
    assert_eq!(progressing.transferred_bytes, 5);
    assert_eq!(progressing.total_bytes, 5);
    assert_command(&root, &["simulate", "complete", "1"]);
    assert_command(&root, &["share", "dismiss", "1"]);
    let completed = operation(&root, "lost-reply")?;
    assert_eq!(completed.phase, Phase::Completed);
    assert!(completed.dismissed);
    assert_eq!(completed.transferred_bytes, 5);
    drop(unread_snapshot);
    fixture.kill()?;
    let restarted = DaemonProcessFixture::start_simulated(root)?;
    assert_eq!(
        operation(restarted.runtime_directory(), "lost-reply")?,
        completed
    );
    let next = client::exchange(
        &socket,
        &Request::submit_text("next", "after-restart"),
    )?;
    let Response::Queued { operation } = next.response() else {
        panic!("expected queued operation after restart");
    };
    assert_eq!(operation.id_string, "2");
    assert_eq!(operation.request_id.as_deref(), Some("after-restart"));
    restarted.stop()
}
