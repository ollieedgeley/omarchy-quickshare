use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use std::env;
use std::fs;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Output, Stdio};
use std::thread;
use std::time::Instant;

use omarchy_quickshare as _;
use quickshare_control::codec::read_response;
use quickshare_control::response::Response;
use quickshare_sharing::{
    Attachment, Direction, DiscoveryState, EndpointSnapshot, Phase,
    VisibilityState,
};

const BINARY: &str = env!("CARGO_BIN_EXE_omarchy-quickshare");
const ACTIVE_TEXT_SNAPSHOT: &str = include_str!(
    "../../../../tests/fixtures/control/v1/active-text-snapshot-response.jsonl"
);
const ACTIVE_FILE_SNAPSHOT: &str = include_str!(
    "../../../../tests/fixtures/control/v1/active-file-snapshot-response.jsonl"
);
const ACTIVE_URL_SNAPSHOT: &str = include_str!(
    "../../../../tests/fixtures/control/v1/active-url-snapshot-response.jsonl"
);
const CANCELLED_TEXT_SNAPSHOT: &str = include_str!(concat!(
    "../../../../tests/fixtures/control/v1/",
    "cancelled-text-snapshot-response.jsonl"
));
const RETRY_DELAY: Duration = Duration::from_millis(5);
const START_TIMEOUT: Duration = Duration::from_secs(1);
static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct DaemonProcessFixture {
    child: Child,
    root: PathBuf,
}

impl DaemonProcessFixture {
    fn kill(mut self) -> io::Result<()> {
        self.child.kill()?;
        let _status = self.child.wait()?;
        Ok(())
    }

    fn runtime_directory(&self) -> &Path {
        &self.root
    }

    fn start(root: PathBuf) -> io::Result<Self> {
        Self::start_with(root, &["--daemon"])
    }

    fn start_simulated(root: PathBuf) -> io::Result<Self> {
        Self::start_with(root, &["--daemon", "--simulate"])
    }

    fn start_with(root: PathBuf, arguments: &[&str]) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let child = Command::new(BINARY)
            .args(arguments)
            .env("XDG_RUNTIME_DIR", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let mut fixture = Self { child, root };
        fixture.wait_until_ready()?;
        Ok(fixture)
    }

    fn stop(mut self) -> io::Result<()> {
        self.child.kill()?;
        let _status = self.child.wait()?;
        fs::remove_dir_all(&self.root)
    }

    fn wait_until_ready(&mut self) -> io::Result<()> {
        let deadline = Instant::now()
            .checked_add(START_TIMEOUT)
            .ok_or_else(|| io::Error::other("startup deadline overflowed"))?;
        while Instant::now() < deadline {
            if matches!(
                runtime_status(&self.root),
                Ok(status) if status.status.success()
            ) {
                return Ok(());
            }
            if self.child.try_wait()?.is_some() {
                return Err(io::Error::other("daemon stopped during startup"));
            }
            thread::sleep(RETRY_DELAY);
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "daemon did not create its control socket",
        ))
    }
}

#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed response preserves the decoded envelope for validation"
)]
fn endpoint_snapshot(runtime_directory: &Path) -> io::Result<EndpointSnapshot> {
    let output = run_command(runtime_directory, &["--status-json"])?;
    if !output.status.success() {
        return Err(io::Error::other("snapshot command failed"));
    }
    let mut reader = BufReader::new(output.stdout.as_slice());
    let envelope = read_response(&mut reader)?;
    match envelope.response() {
        Response::Snapshot { snapshot } => Ok(snapshot.clone()),
        Response::Cancelled
        | Response::NotFound
        | Response::Queued { .. }
        | Response::Ready
        | _ => Err(io::Error::other("endpoint did not return a snapshot")),
    }
}

fn runtime_fixture_path() -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "omarchy-quickshare-process-{}-{sequence}",
        process::id()
    ))
}

fn runtime_status(runtime_directory: &Path) -> io::Result<Output> {
    Command::new(BINARY)
        .arg("--runtime-status")
        .env("XDG_RUNTIME_DIR", runtime_directory)
        .output()
}

fn run_command(
    runtime_directory: &Path,
    arguments: &[&str],
) -> io::Result<Output> {
    Command::new(BINARY)
        .args(arguments)
        .env("XDG_RUNTIME_DIR", runtime_directory)
        .output()
}

#[test]
fn daemon_process_accepts_clients_and_recovers_from_a_stale_socket() {
    let root = runtime_fixture_path();
    let first_result = DaemonProcessFixture::start(root.clone());
    assert!(
        first_result.is_ok(),
        "failed to start daemon: {first_result:?}"
    );
    let Ok(first) = first_result else {
        return;
    };
    let status_result = runtime_status(first.runtime_directory());
    assert!(status_result.is_ok(), "failed to query daemon status");
    let Ok(status) = status_result else {
        return;
    };
    assert!(status.status.success(), "daemon rejected status request");
    assert_eq!(status.stdout, b"available\n");
    let first_stop = first.kill();
    assert!(first_stop.is_ok(), "failed to stop first daemon");

    let second_result = DaemonProcessFixture::start(root);
    assert!(second_result.is_ok(), "failed to restart daemon");
    let Ok(second) = second_result else {
        return;
    };
    let second_status_result = runtime_status(second.runtime_directory());
    assert!(
        second_status_result.is_ok(),
        "failed to query restarted daemon"
    );
    let Ok(second_status) = second_status_result else {
        return;
    };
    assert!(second_status.status.success());
    let second_stop = second.stop();
    assert!(second_stop.is_ok(), "failed to stop restarted daemon");
}

#[test]
fn daemon_reports_submitted_text_in_its_public_snapshot() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start(root);
    assert!(fixture_result.is_ok(), "failed to start daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };

    let submission_result =
        run_command(fixture.runtime_directory(), &["hello from Omarchy"]);
    assert!(submission_result.is_ok(), "failed to submit text");
    let Ok(submission) = submission_result else {
        return;
    };
    assert!(submission.status.success(), "daemon rejected text");

    let snapshot_result =
        run_command(fixture.runtime_directory(), &["--status-json"]);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    assert!(
        snapshot.status.success(),
        "daemon rejected snapshot request"
    );
    assert_eq!(snapshot.stdout, ACTIVE_TEXT_SNAPSHOT.as_bytes());

    let stop_result = fixture.stop();
    assert!(stop_result.is_ok(), "failed to stop daemon");
}

#[test]
fn daemon_reports_submitted_url_in_its_public_snapshot() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start(root);
    assert!(fixture_result.is_ok(), "failed to start daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };

    let submission_result = run_command(
        fixture.runtime_directory(),
        &["https://example.test/share"],
    );
    assert!(submission_result.is_ok(), "failed to submit URL");
    let snapshot_result =
        run_command(fixture.runtime_directory(), &["--status-json"]);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    assert_eq!(snapshot.stdout, ACTIVE_URL_SNAPSHOT.as_bytes());

    let stop_result = fixture.stop();
    assert!(stop_result.is_ok(), "failed to stop daemon");
}

#[test]
fn daemon_reports_submitted_file_in_its_public_snapshot() {
    let root = runtime_fixture_path();
    let source = root.join("photo.jpg");
    let directory_result = fs::create_dir_all(&root);
    assert!(
        directory_result.is_ok(),
        "failed to create fixture directory"
    );
    let write_result = fs::write(&source, b"jpeg");
    assert!(write_result.is_ok(), "failed to create fixture file");
    let fixture_result = DaemonProcessFixture::start(root);
    assert!(fixture_result.is_ok(), "failed to start daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };

    let submission_result = run_command(
        fixture.runtime_directory(),
        &[source.to_string_lossy().as_ref()],
    );
    assert!(submission_result.is_ok(), "failed to submit file");
    let snapshot_result =
        run_command(fixture.runtime_directory(), &["--status-json"]);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    assert_eq!(snapshot.stdout, ACTIVE_FILE_SNAPSHOT.as_bytes());

    let stop_result = fixture.stop();
    assert!(stop_result.is_ok(), "failed to stop daemon");
}

#[test]
fn daemon_cancels_the_active_share_by_identifier() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start(root);
    assert!(fixture_result.is_ok(), "failed to start daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    let submission_result =
        run_command(fixture.runtime_directory(), &["hello"]);
    assert!(submission_result.is_ok(), "failed to submit text");

    let cancellation_result =
        run_command(fixture.runtime_directory(), &["--cancel", "1"]);
    assert!(cancellation_result.is_ok(), "failed to run cancellation");
    let Ok(cancellation) = cancellation_result else {
        return;
    };
    assert!(
        cancellation.status.success(),
        "daemon rejected cancellation"
    );
    assert_eq!(cancellation.stdout, b"Share cancelled.\n");
    let snapshot_result =
        run_command(fixture.runtime_directory(), &["--status-json"]);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    assert_eq!(snapshot.stdout, CANCELLED_TEXT_SNAPSHOT.as_bytes());

    let stop_result = fixture.stop();
    assert!(stop_result.is_ok(), "failed to stop daemon");
}

#[test]
fn simulated_daemon_runs_an_outbound_transfer_with_a_pinned_peer() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    let initial_result = endpoint_snapshot(fixture.runtime_directory());
    assert!(initial_result.is_ok(), "failed to read initial snapshot");
    let Ok(initial) = initial_result else {
        return;
    };
    assert_eq!(initial.peers().len(), 2);

    assert_command(fixture.runtime_directory(), &["--pin", "galaxy-tab"]);
    assert_command(fixture.runtime_directory(), &["hello"]);
    assert_active_peer(
        fixture.runtime_directory(),
        "galaxy-tab",
        Phase::AwaitingPeerConsent,
    );
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-peer-accept", "1"],
    );
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-progress", "1", "5"],
    );
    assert_phase(
        fixture.runtime_directory(),
        Direction::Outbound,
        Phase::Completed,
    );
    let stop_result = fixture.stop();
    assert!(stop_result.is_ok(), "failed to stop simulated daemon");
}

#[test]
fn simulated_daemon_runs_an_inbound_transfer() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-incoming-text", "from phone"],
    );
    assert_active_peer(
        fixture.runtime_directory(),
        "pixel-8",
        Phase::AwaitingLocalConsent,
    );
    assert_command(fixture.runtime_directory(), &["--accept", "1"]);
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-progress", "1", "10"],
    );
    assert_phase(
        fixture.runtime_directory(),
        Direction::Inbound,
        Phase::Completed,
    );
    let stop_result = fixture.stop();
    assert!(stop_result.is_ok(), "failed to stop simulated daemon");
}

#[test]
fn simulated_daemon_exposes_peer_rejection_and_dismissal() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    assert_command(fixture.runtime_directory(), &["hello"]);
    assert_command(fixture.runtime_directory(), &["--send-to", "1", "pixel-8"]);
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-peer-reject", "1"],
    );
    assert_phase(
        fixture.runtime_directory(),
        Direction::Outbound,
        Phase::Rejected,
    );
    assert_command(fixture.runtime_directory(), &["--dismiss", "1"]);
    assert_idle(fixture.runtime_directory());
    assert!(fixture.stop().is_ok(), "failed to stop simulated daemon");
}

#[test]
fn simulated_daemon_exposes_failure_and_dismissal() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    assert_command(fixture.runtime_directory(), &["hello"]);
    assert_command(fixture.runtime_directory(), &["--send-to", "1", "pixel-8"]);
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-peer-accept", "1"],
    );
    assert_command(fixture.runtime_directory(), &["--simulate-fail", "1"]);
    assert_phase(
        fixture.runtime_directory(),
        Direction::Outbound,
        Phase::Failed,
    );
    assert_command(fixture.runtime_directory(), &["--dismiss", "1"]);
    assert_idle(fixture.runtime_directory());
    assert!(fixture.stop().is_ok(), "failed to stop simulated daemon");
}

#[test]
fn simulated_daemon_changes_visible_peers() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-peer-lost", "pixel-8"],
    );
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-peer-lost", "galaxy-tab"],
    );
    assert_peer_ids(fixture.runtime_directory(), &[]);
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-peer-seen", "watch-7", "Pixel Watch"],
    );
    assert_peer_ids(fixture.runtime_directory(), &["watch-7"]);
    assert!(fixture.stop().is_ok(), "failed to stop simulated daemon");
}

#[test]
fn simulated_daemon_drives_discovery_and_visibility() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    assert_command(fixture.runtime_directory(), &["--discover"]);
    assert_endpoint_modes(
        fixture.runtime_directory(),
        DiscoveryState::Searching,
        VisibilityState::Closed,
    );
    assert_command(fixture.runtime_directory(), &["--open-visibility"]);
    assert_endpoint_modes(
        fixture.runtime_directory(),
        DiscoveryState::Searching,
        VisibilityState::Open,
    );
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-discovery-timeout"],
    );
    assert_endpoint_modes(
        fixture.runtime_directory(),
        DiscoveryState::TimedOut,
        VisibilityState::Open,
    );
    assert_command(fixture.runtime_directory(), &["--discover"]);
    assert_command(fixture.runtime_directory(), &["--stop-discovery"]);
    assert_command(fixture.runtime_directory(), &["--close-visibility"]);
    assert_endpoint_modes(
        fixture.runtime_directory(),
        DiscoveryState::Idle,
        VisibilityState::Closed,
    );
    assert!(fixture.stop().is_ok(), "failed to stop simulated daemon");
}

#[test]
fn simulated_daemon_offers_url_and_file_attachments() {
    let root = runtime_fixture_path();
    let fixture_result = DaemonProcessFixture::start_simulated(root);
    assert!(fixture_result.is_ok(), "failed to start simulated daemon");
    let Ok(fixture) = fixture_result else {
        return;
    };
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-incoming-url", "https://example.test/from-phone"],
    );
    assert_attachment(
        fixture.runtime_directory(),
        &Attachment::url("https://example.test/from-phone"),
    );
    assert_command(fixture.runtime_directory(), &["--reject", "1"]);
    assert_command(fixture.runtime_directory(), &["--dismiss", "1"]);
    assert_command(
        fixture.runtime_directory(),
        &["--simulate-incoming-file", "photo.jpg", "1024"],
    );
    assert_attachment(
        fixture.runtime_directory(),
        &Attachment::file("photo.jpg", 1024),
    );
    assert!(fixture.stop().is_ok(), "failed to stop simulated daemon");
}

fn assert_command(runtime_directory: &Path, arguments: &[&str]) {
    let result = run_command(runtime_directory, arguments);
    assert!(result.is_ok(), "failed to run {arguments:?}");
    let Ok(output) = result else {
        return;
    };
    assert!(output.status.success(), "command rejected: {arguments:?}");
}

fn assert_endpoint_modes(
    runtime_directory: &Path,
    discovery: DiscoveryState,
    visibility: VisibilityState,
) {
    let snapshot_result = endpoint_snapshot(runtime_directory);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    assert_eq!(snapshot.discovery(), discovery);
    assert_eq!(snapshot.visibility(), visibility);
}

fn assert_active_peer(runtime_directory: &Path, peer_id: &str, phase: Phase) {
    let snapshot_result = endpoint_snapshot(runtime_directory);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    let active_result = snapshot.active_share();
    assert!(active_result.is_some(), "active share is not visible");
    let Some(active) = active_result else {
        return;
    };
    let peer_result = active.peer();
    assert!(peer_result.is_some(), "active peer is not visible");
    let Some(peer) = peer_result else {
        return;
    };
    assert_eq!(peer.id(), peer_id);
    assert_eq!(active.phase(), phase);
}

fn assert_attachment(runtime_directory: &Path, attachment: &Attachment) {
    let snapshot_result = endpoint_snapshot(runtime_directory);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    let active_result = snapshot.active_share();
    assert!(active_result.is_some(), "active share is not visible");
    let Some(active) = active_result else {
        return;
    };
    assert_eq!(active.attachment(), attachment);
}

fn assert_idle(runtime_directory: &Path) {
    let snapshot_result = endpoint_snapshot(runtime_directory);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    assert!(snapshot.active_share().is_none());
}

fn assert_peer_ids(runtime_directory: &Path, expected: &[&str]) {
    let snapshot_result = endpoint_snapshot(runtime_directory);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    let ids = snapshot
        .peers()
        .iter()
        .map(quickshare_sharing::PeerSnapshot::id)
        .collect::<Vec<_>>();
    assert_eq!(ids, expected);
}

fn assert_phase(runtime_directory: &Path, direction: Direction, phase: Phase) {
    let snapshot_result = endpoint_snapshot(runtime_directory);
    assert!(snapshot_result.is_ok(), "failed to read endpoint snapshot");
    let Ok(snapshot) = snapshot_result else {
        return;
    };
    let active_result = snapshot.active_share();
    assert!(active_result.is_some(), "active share is not visible");
    let Some(active) = active_result else {
        return;
    };
    assert_eq!(active.direction(), direction);
    assert_eq!(active.phase(), phase);
}
