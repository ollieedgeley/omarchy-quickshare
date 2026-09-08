#![expect(
    clippy::panic_in_result_fn,
    reason = "Tests propagate I/O setup errors and assert public behavior"
)]

#[path = "config_contract/live_preferences.rs"]
mod live_preferences;

use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Output, Stdio};
use std::thread;
use std::time::Instant;

use omarchy_quickshare as _;
use quickshare_control::codec::read_response;
use quickshare_control::response::Response;
use quickshare_network as _;
use quickshare_sharing::{Attachment, EndpointSnapshot};

const DAEMON_BINARY: &str = env!("CARGO_BIN_EXE_omarchy-quickshare-daemon");
const RETRY_DELAY: Duration = Duration::from_millis(5);
const START_TIMEOUT: Duration = Duration::from_secs(1);
static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    child: Child,
    root: PathBuf,
}

impl Fixture {
    fn start(root: PathBuf) -> io::Result<Self> {
        Self::start_with(root, &["--simulate"])
    }

    fn start_with(root: PathBuf, arguments: &[&str]) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let child = daemon_command(&root, arguments)
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
                command(&self.root, &["health"]).output(),
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
            "daemon did not become ready",
        ))
    }
}

fn command(root: &Path, arguments: &[&str]) -> Command {
    process_command(
        &Path::new(DAEMON_BINARY).with_file_name("omarchy-quickshare"),
        root,
        arguments,
    )
}

fn daemon_command(root: &Path, arguments: &[&str]) -> Command {
    process_command(Path::new(DAEMON_BINARY), root, arguments)
}

fn process_command(binary: &Path, root: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(binary);
    let _ = command
        .args(arguments)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_RUNTIME_DIR", root)
        .env("OMARCHY_QUICKSHARE_DISABLE_NOTIFICATIONS", "1")
        .env_remove("XDG_DOWNLOAD_DIR");
    command
}

fn fixture_root() -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "omarchy-quickshare-config-{}-{sequence}",
        process::id()
    ))
}

fn run(root: &Path, arguments: &[&str]) -> io::Result<Output> {
    command(root, arguments).output()
}

fn snapshot(root: &Path) -> io::Result<EndpointSnapshot> {
    let output = run(root, &["status", "--json"])?;
    if !output.status.success() {
        return Err(io::Error::other("snapshot command failed"));
    }
    let envelope = read_response(&mut output.stdout.as_slice())?;
    match envelope.response() {
        Response::Snapshot { snapshot, .. } => Ok(snapshot.clone()),
        Response::Applied
        | Response::Cancelled
        | Response::NotFound
        | Response::Queued { .. }
        | Response::Ready
        | _ => Err(io::Error::other("expected snapshot")),
    }
}

#[test]
fn durable_discoverable_activates_on_daemon_start() -> io::Result<()> {
    let root = fixture_root();
    let config_directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&config_directory)?;
    fs::write(
        config_directory.join("config.toml"),
        "discoverable = true\n",
    )?;
    let fixture = Fixture::start(root)?;
    let endpoint = snapshot(&fixture.root)?;
    fixture.stop()?;
    assert_eq!(
        endpoint.visibility(),
        quickshare_sharing::VisibilityState::Open
    );
    Ok(())
}

#[test]
fn live_preferences_separates_legacy_consent_timeout() -> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    let path = directory.join("config.toml");
    let legacy = "# Legacy deadline\nvisibility_timeout_secs = 17\n";
    fs::write(&path, legacy)?;
    let before = run(&root, &["config", "show"])?;
    assert!(before.status.success());
    assert_eq!(
        fs::read_to_string(&path)?,
        legacy,
        "reading must not migrate the file"
    );
    let visibility =
        run(&root, &["config", "set", "visibility_timeout_secs", "31"])?;
    assert!(!visibility.status.success());
    let device = run(&root, &["config", "set", "device_name", "patched"])?;
    assert!(device.status.success());
    let inspected = run(&root, &["config", "show"])?;
    assert!(inspected.status.success());
    let body = String::from_utf8_lossy(&inspected.stdout);
    assert!(body.contains("consent_timeout_secs = 17"));
    assert!(fs::read_to_string(&path)?.contains("# Legacy deadline"));
    let consent = run(&root, &["config", "set", "consent_timeout_secs", "42"])?;
    assert!(consent.status.success());
    let updated = run(&root, &["config", "show"])?;
    assert!(updated.status.success());
    let body = String::from_utf8_lossy(&updated.stdout);
    assert!(body.contains("consent_timeout_secs = 42"));
    let fixture = Fixture::start(root)?;
    let opened = run(&fixture.root, &["visibility", "open"])?;
    assert!(opened.status.success());
    let endpoint = snapshot(&fixture.root)?;
    fixture.stop()?;
    assert!(
        endpoint
            .visibility_status()
            .remaining_secs
            .is_some_and(|remaining| remaining > 300 && remaining <= 600)
    );
    Ok(())
}

#[test]
fn simulation_commands_are_unavailable_without_explicit_opt_in() {
    let root = fixture_root();
    let fixture = Fixture::start(root);
    assert!(fixture.is_ok(), "daemon");
    let Ok(fixture) = fixture else {
        return;
    };
    let output = run(
        fixture.root.as_path(),
        &["simulate", "peer-seen", "watch-7", "Watch"],
    );
    assert!(output.is_ok(), "simulate command");
    let Ok(output) = output else {
        return;
    };
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("simulation commands are unavailable"));
    let stopped = fixture.stop();
    assert!(stopped.is_ok(), "stop");
}

#[test]
fn folder_submit_queues_a_zip_and_status_exposes_share_observations() {
    let root = fixture_root();
    let created = fs::create_dir_all(&root);
    assert!(created.is_ok(), "fixture root");
    let folder = root.join("album");
    let folder_created = fs::create_dir_all(&folder);
    assert!(folder_created.is_ok(), "folder");
    let written = fs::write(folder.join("note.txt"), b"hi");
    assert!(written.is_ok(), "file");
    let fixture = Fixture::start(root.clone());
    assert!(fixture.is_ok(), "daemon");
    let Ok(fixture) = fixture else {
        return;
    };
    let Some(folder_path) = folder.to_str() else {
        return;
    };
    let submitted = run(fixture.root.as_path(), &["send", folder_path]);
    assert!(submitted.is_ok(), "submit folder");
    let Ok(submitted) = submitted else {
        return;
    };
    assert!(
        submitted.status.success(),
        "folder submit failed: {}",
        String::from_utf8_lossy(&submitted.stderr)
    );
    let snapshot = snapshot(fixture.root.as_path());
    assert!(snapshot.is_ok(), "snapshot");
    let Ok(snapshot) = snapshot else {
        return;
    };
    let Some(active) = snapshot.active_share() else {
        return;
    };
    let name = match active.attachment() {
        Attachment::File { name, .. } => name.as_str(),
        Attachment::Text { .. } | Attachment::Url { .. } | _ => "",
    };
    assert!(name.ends_with(".zip"), "queued {name}");
    let status = run(fixture.root.as_path(), &["status"]);
    assert!(status.is_ok(), "status");
    let Ok(status) = status else {
        return;
    };
    let body = String::from_utf8_lossy(&status.stdout);
    assert!(body.contains("direction=outbound"));
    assert!(body.contains("transferred_bytes=0"));
    let stopped = fixture.stop();
    assert!(stopped.is_ok(), "stop");
}

#[test]
fn pin_persists_and_unpin_clears_the_single_peer() {
    let root = fixture_root();
    let fixture = Fixture::start(root);
    assert!(fixture.is_ok(), "daemon");
    let Ok(fixture) = fixture else {
        return;
    };
    let pinned = run(fixture.root.as_path(), &["peer", "pin", "galaxy-tab"]);
    assert!(pinned.is_ok(), "pin");
    let Ok(pinned) = pinned else {
        return;
    };
    assert!(pinned.status.success(), "pin failed");
    let inspect = run(fixture.root.as_path(), &["config", "show"]);
    assert!(inspect.is_ok(), "config");
    let Ok(inspect) = inspect else {
        return;
    };
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(body.contains("galaxy-tab"));
    let unpinned = run(fixture.root.as_path(), &["peer", "unpin"]);
    assert!(unpinned.is_ok(), "unpin");
    let Ok(unpinned) = unpinned else {
        return;
    };
    assert!(unpinned.status.success(), "unpin failed");
    let snapshot = snapshot(fixture.root.as_path());
    assert!(snapshot.is_ok(), "snapshot");
    let Ok(snapshot) = snapshot else {
        return;
    };
    assert!(snapshot.peers().iter().all(|peer| !peer.is_pinned()));
    let inspect = run(fixture.root.as_path(), &["config", "show"]);
    assert!(inspect.is_ok(), "config after unpin");
    let Ok(inspect) = inspect else {
        return;
    };
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(!body.contains("galaxy-tab"));
    let stopped = fixture.stop();
    assert!(stopped.is_ok(), "stop");
}
