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

const BINARY: &str = env!("CARGO_BIN_EXE_omarchy-quickshare");
const RETRY_DELAY: Duration = Duration::from_millis(5);
const START_TIMEOUT: Duration = Duration::from_secs(1);
static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    child: Child,
    root: PathBuf,
}

impl Fixture {
    fn start(root: PathBuf) -> io::Result<Self> {
        Self::start_with(root, &["--daemon", "--simulate"])
    }

    fn start_with(root: PathBuf, arguments: &[&str]) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        let child = command(&root, arguments)
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
                command(&self.root, &["--runtime-status"]).output(),
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
    let mut command = Command::new(BINARY);
    let _ = command
        .args(arguments)
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_RUNTIME_DIR", root)
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
    let output = run(root, &["--status-json"])?;
    if !output.status.success() {
        return Err(io::Error::other("snapshot command failed"));
    }
    let envelope = read_response(&mut output.stdout.as_slice())?;
    match envelope.response() {
        Response::Snapshot { snapshot } => Ok(snapshot.clone()),
        Response::Applied
        | Response::Cancelled
        | Response::NotFound
        | Response::Queued { .. }
        | Response::Ready
        | _ => Err(io::Error::other("expected snapshot")),
    }
}

#[test]
fn missing_config_uses_documented_defaults() {
    let root = fixture_root();
    let created = fs::create_dir_all(&root);
    assert!(created.is_ok(), "fixture root");
    let output = run(&root, &["--config"]);
    assert!(output.is_ok(), "config inspect");
    let Ok(output) = output else {
        return;
    };
    assert!(output.status.success(), "config inspect failed");
    let body = String::from_utf8_lossy(&output.stdout);
    let fallback = root.join("Downloads/omarchy-quickshare");
    assert!(
        body.contains(&format!(
            "receive_directory = \"{}\"",
            fallback.display()
        ))
    );
    assert!(body.contains("discovery_timeout_secs = 15"));
    assert!(body.contains("visibility_timeout_secs = 300"));
    assert!(body.contains("transfer_timeout_secs = 120"));
    let cleaned = fs::remove_dir_all(&root);
    assert!(cleaned.is_ok(), "cleanup");
}

#[test]
fn missing_config_uses_xdg_download_dir_when_set() {
    let root = fixture_root();
    let created = fs::create_dir_all(&root);
    assert!(created.is_ok(), "fixture root");

    let mut xdg = command(&root, &["--config"]);
    let _ = xdg.env("XDG_DOWNLOAD_DIR", "/cases/received");
    let xdg_output = xdg.output();
    assert!(xdg_output.is_ok(), "xdg config inspect");
    let Ok(xdg_output) = xdg_output else {
        return;
    };
    assert!(xdg_output.status.success(), "xdg config inspect failed");
    let xdg_body = String::from_utf8_lossy(&xdg_output.stdout);
    assert!(xdg_body.contains(
        "receive_directory = \"/cases/received/omarchy-quickshare\""
    ));

    let mut empty = command(&root, &["--config"]);
    let _ = empty.env("XDG_DOWNLOAD_DIR", "");
    let empty_output = empty.output();
    assert!(empty_output.is_ok(), "empty xdg config inspect");
    let Ok(empty_output) = empty_output else {
        return;
    };
    assert!(
        empty_output.status.success(),
        "empty xdg config inspect failed"
    );
    let empty_body = String::from_utf8_lossy(&empty_output.stdout);
    let fallback = root.join("Downloads/omarchy-quickshare");
    assert!(
        empty_body.contains(&format!(
            "receive_directory = \"{}\"",
            fallback.display()
        ))
    );

    let cleaned = fs::remove_dir_all(&root);
    assert!(cleaned.is_ok(), "cleanup");
}

#[test]
fn config_set_persists_and_rejects_unknown_keys() {
    let root = fixture_root();
    let created = fs::create_dir_all(&root);
    assert!(created.is_ok(), "fixture root");
    let receive = root.join("inbox");
    let Some(receive_path) = receive.to_str() else {
        return;
    };
    let updated =
        run(&root, &["--config-set", "receive_directory", receive_path]);
    assert!(updated.is_ok(), "config-set");
    let Ok(updated) = updated else {
        return;
    };
    assert!(updated.status.success(), "config-set failed");
    let inspect = run(&root, &["--config"]);
    assert!(inspect.is_ok(), "config inspect");
    let Ok(inspect) = inspect else {
        return;
    };
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(body.contains(&receive.display().to_string()));
    let unknown = run(&root, &["--config-set", "not_a_key", "1"]);
    assert!(unknown.is_ok(), "unknown key");
    let Ok(unknown) = unknown else {
        return;
    };
    assert!(!unknown.status.success());
    let stderr = String::from_utf8_lossy(&unknown.stderr);
    assert!(stderr.contains("unknown config key"));
    let hostile = fs::write(
        root.join("config/omarchy-quickshare/config.toml"),
        "mystery = 1\n",
    );
    assert!(hostile.is_ok(), "hostile config");
    let invalid = run(&root, &["--config"]);
    assert!(invalid.is_ok(), "invalid config");
    let Ok(invalid) = invalid else {
        return;
    };
    assert!(!invalid.status.success());
    let cleaned = fs::remove_dir_all(&root);
    assert!(cleaned.is_ok(), "cleanup");
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
        &["--simulate-peer-seen", "watch-7", "Watch"],
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
    let submitted = run(fixture.root.as_path(), &[folder_path]);
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
    let status = run(fixture.root.as_path(), &["--status"]);
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
    let pinned = run(fixture.root.as_path(), &["--pin", "galaxy-tab"]);
    assert!(pinned.is_ok(), "pin");
    let Ok(pinned) = pinned else {
        return;
    };
    assert!(pinned.status.success(), "pin failed");
    let inspect = run(fixture.root.as_path(), &["--config"]);
    assert!(inspect.is_ok(), "config");
    let Ok(inspect) = inspect else {
        return;
    };
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(body.contains("galaxy-tab"));
    let unpinned = run(fixture.root.as_path(), &["--unpin"]);
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
    let inspect = run(fixture.root.as_path(), &["--config"]);
    assert!(inspect.is_ok(), "config after unpin");
    let Ok(inspect) = inspect else {
        return;
    };
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(!body.contains("galaxy-tab"));
    let stopped = fixture.stop();
    assert!(stopped.is_ok(), "stop");
}
