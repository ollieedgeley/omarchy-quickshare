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
use quickshare_control as _;

const BINARY: &str = env!("CARGO_BIN_EXE_omarchy-quickshare");
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
        fs::create_dir_all(&root)?;
        let child = Command::new(BINARY)
            .arg("--daemon")
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
        let socket = self.root.join("omarchy-quickshare/control.sock");
        let deadline = Instant::now()
            .checked_add(START_TIMEOUT)
            .ok_or_else(|| io::Error::other("startup deadline overflowed"))?;
        while Instant::now() < deadline {
            if socket.exists() {
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
    clippy::single_call_fn,
    reason = "The process fixture path construction stays named in the test"
)]
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
