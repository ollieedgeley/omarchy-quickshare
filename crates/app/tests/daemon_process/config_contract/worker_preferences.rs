use super::super::{Fixture, RETRY_DELAY, command, fixture_root, snapshot};
use core::time::Duration;
use quickshare_sharing::{Phase, SharingSession};
use std::env;
use std::fs;
use std::io::{self, Cursor, Read};
use std::net::{TcpListener, TcpStream};
use std::panic::{catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Instant;

const PAYLOAD: &[u8] = b"admitted file bytes";
const WAIT: Duration = Duration::from_secs(4);

struct GatedReader {
    bytes: Cursor<&'static [u8]>,
    gate: Option<(Sender<()>, Receiver<()>)>,
}

#[expect(
    clippy::missing_trait_methods,
    reason = "default Read methods must pass through the gated read"
)]
impl Read for GatedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some((accepted, release)) = self.gate.take() {
            accepted.send(()).map_err(io::Error::other)?;
            release.recv_timeout(WAIT).map_err(io::Error::other)?;
        }
        self.bytes.read(buf)
    }
}

fn checked(root: &Path, arguments: &[&str]) -> io::Result<()> {
    let output = command(root, arguments).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }
}

fn wait_phase(root: &Path, phase: Phase) -> io::Result<u64> {
    let deadline = Instant::now()
        .checked_add(WAIT)
        .ok_or_else(|| io::Error::other("phase deadline overflow"))?;
    loop {
        let state = snapshot(root)?;
        if let Some(share) = state.active_share()
            && share.phase() == phase
        {
            return Ok(share.id().get());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("expected {phase:?}, observed {state:?}"),
            ));
        }
        thread::sleep(RETRY_DELAY);
    }
}

fn send_file(
    port: u16,
    name: &'static str,
    gate: Option<(Sender<()>, Receiver<()>)>,
) -> JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let deadline = Instant::now()
            .checked_add(WAIT)
            .ok_or_else(|| io::Error::other("sender deadline overflow"))?;
        let stream = loop {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(stream) => break stream,
                Err(error)
                    if error.kind() == io::ErrorKind::ConnectionRefused
                        && Instant::now() < deadline =>
                {
                    thread::sleep(RETRY_DELAY);
                }
                Err(error) => return Err(error),
            }
        };
        stream.set_read_timeout(Some(WAIT))?;
        stream.set_write_timeout(Some(WAIT))?;
        let mut session =
            SharingSession::connect_io(stream, "TEST", "Fixture peer")
                .map_err(io::Error::other)?;
        let _pairing = session
            .exchange_account_free_pairing()
            .map_err(io::Error::other)?;
        let mut reader = GatedReader {
            bytes: Cursor::new(PAYLOAD),
            gate,
        };
        session
            .send_outgoing_file(
                name,
                u64::try_from(PAYLOAD.len()).map_err(io::Error::other)?,
                &mut reader,
                || {},
                |_| {},
                || false,
            )
            .map_err(io::Error::other)?;
        Ok(())
    })
}

fn join_sender(sender: JoinHandle<io::Result<()>>) -> io::Result<()> {
    sender
        .join()
        .map_err(|_panic| io::Error::other("sender panicked"))?
}

fn accept(root: &Path) -> io::Result<u64> {
    let share_id = wait_phase(root, Phase::AwaitingLocalConsent)?;
    checked(root, &["share", "accept", &share_id.to_string()])?;
    Ok(share_id)
}

fn open_visibility(root: &Path) -> io::Result<()> {
    checked(root, &["visibility", "open"])?;
    let deadline = Instant::now()
        .checked_add(WAIT)
        .ok_or_else(|| io::Error::other("visibility deadline overflow"))?;
    while !matches!(
        snapshot(root)?.visibility(),
        quickshare_sharing::VisibilityState::Open
    ) {
        if Instant::now() >= deadline {
            return Err(io::Error::other("visibility did not open"));
        }
        thread::sleep(RETRY_DELAY);
    }
    Ok(())
}

fn set_receive_directory(root: &Path, directory: &Path) -> io::Result<()> {
    checked(
        root,
        &[
            "config",
            "set",
            "receive_directory",
            &directory.to_string_lossy(),
        ],
    )
}

fn assert_received_and_dismiss(
    root: &Path,
    share_id: u64,
    name: &str,
    directory: &Path,
    unused_directory: &Path,
) -> io::Result<()> {
    let _completed = wait_phase(root, Phase::Completed)?;
    assert_eq!(fs::read(directory.join(name))?, PAYLOAD);
    assert!(!unused_directory.join(name).exists());
    checked(root, &["share", "dismiss", &share_id.to_string()])
}

fn assert_timed_out(root: &Path) -> io::Result<()> {
    assert_eq!(
        snapshot(root)?
            .active_share()
            .and_then(|share| share.terminal_reason()),
        Some("timed_out")
    );
    Ok(())
}

fn admitted_settings(root: &Path, port: u16) -> io::Result<()> {
    let old_directory = root.join("old");
    let new_directory = root.join("new");
    open_visibility(root)?;
    let (accepted, ready) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let first = send_file(port, "first.txt", Some((accepted, released)));
    let first_id = accept(root)?;
    ready.recv_timeout(WAIT).map_err(io::Error::other)?;
    set_receive_directory(root, &new_directory)?;
    checked(root, &["config", "set", "transfer_timeout_secs", "1"])?;
    checked(root, &["config", "set", "device_name", "Renamed endpoint"])?;
    thread::sleep(Duration::from_millis(1200));
    release.send(()).map_err(io::Error::other)?;
    join_sender(first)?;
    assert_received_and_dismiss(
        root,
        first_id,
        "first.txt",
        &old_directory,
        &new_directory,
    )?;

    let second = send_file(port, "second.txt", None);
    let second_id = accept(root)?;
    join_sender(second)?;
    assert_received_and_dismiss(
        root,
        second_id,
        "second.txt",
        &new_directory,
        &old_directory,
    )?;

    let (accepted, ready) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let third = send_file(port, "expired.txt", Some((accepted, released)));
    let _third_id = accept(root)?;
    ready.recv_timeout(WAIT).map_err(io::Error::other)?;
    let _failed = wait_phase(root, Phase::Failed)?;
    release.send(()).map_err(io::Error::other)?;
    let _sender_result = join_sender(third);
    assert_timed_out(root)?;
    assert!(!new_directory.join("expired.txt").exists());
    Ok(())
}

fn isolated_worker(
    test_name: &str,
    scenario: fn(&Path, u16) -> io::Result<()>,
) -> io::Result<()> {
    if env::var_os("OMARCHY_QUICKSHARE_PRIVATE_NETWORK").is_none() {
        return reexec_in_private_network(test_name);
    }
    setup_private_network()?;
    run_daemon_scenario(scenario)
}

fn reexec_in_private_network(test_name: &str) -> io::Result<()> {
    let output = Command::new("unshare")
        .args(["--user", "--map-root-user", "--net"])
        .arg(env::current_exe()?)
        .args([
            "--exact",
            &format!(
                "config_contract::live_preferences::\
                 worker_preferences::{test_name}"
            ),
            "--nocapture",
        ])
        .env(
            "OMARCHY_QUICKSHARE_PRIVATE_NETWORK",
            fs::read_link("/proc/self/ns/net")?,
        )
        .output()?;
    assert!(
        output.status.success(),
        "isolated worker contract failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn setup_private_network() -> io::Result<()> {
    assert_ne!(
        fs::read_link("/proc/self/ns/net")?,
        env::var_os("OMARCHY_QUICKSHARE_PRIVATE_NETWORK")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other(
                "missing parent namespace identity"
            ))?,
        "refusing host network setup"
    );
    assert!(
        Command::new("ip")
            .args(["link", "set", "lo", "up"])
            .status()?
            .success()
    );
    assert!(
        Command::new("ip")
            .args(["address", "add", "192.0.2.1/32", "dev", "lo"])
            .status()?
            .success()
    );
    let setup: [&[&str]; 5] = [
        &[
            "link",
            "add",
            "fixture-a",
            "type",
            "veth",
            "peer",
            "name",
            "fixture-b",
        ],
        &["address", "add", "192.0.2.2/24", "dev", "fixture-a"],
        &["address", "add", "192.0.2.3/24", "dev", "fixture-b"],
        &["link", "set", "fixture-a", "up"],
        &["link", "set", "fixture-b", "up"],
    ];
    for arguments in setup {
        assert!(
            Command::new("ip").args(arguments).status()?.success(),
            "private interface setup"
        );
    }
    Ok(())
}

fn run_daemon_scenario(
    scenario: fn(&Path, u16) -> io::Result<()>,
) -> io::Result<()> {
    let root = fixture_root();
    let config_directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&config_directory)?;
    fs::write(
        config_directory.join("config.toml"),
        format!(
            concat!(
                "device_name = \"Original endpoint\"\n",
                "receive_directory = \"{}\"\ntransfer_timeout_secs = 3\n"
            ),
            root.join("old").display()
        ),
    )?;
    let reserved = TcpListener::bind("127.0.0.1:0")?;
    let port = reserved.local_addr()?.port();
    drop(reserved);
    let child = command(&root, &["daemon"])
        .env(
            "DBUS_SYSTEM_BUS_ADDRESS",
            format!("unix:path={}/missing-bus", root.display()),
        )
        .env("OMARCHY_QUICKSHARE_TEST_LAN_PORT", port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let mut fixture = Fixture {
        child,
        root: root.clone(),
    };
    let result = fixture
        .wait_until_ready()
        .map(|()| catch_unwind(|| scenario(&root, port)));
    if fixture.child.try_wait()?.is_none() {
        fixture.stop()?;
    } else {
        fs::remove_dir_all(root)?;
    }
    match result? {
        Ok(outcome) => outcome,
        Err(panic) => resume_unwind(panic),
    }
}

#[test]
fn live_preferences_worker_preserves_admitted_settings() -> io::Result<()> {
    isolated_worker(
        "live_preferences_worker_preserves_admitted_settings",
        admitted_settings,
    )
}

fn pending_admitted_settings(root: &Path, port: u16) -> io::Result<()> {
    let old_directory = root.join("old");
    let new_directory = root.join("new");
    checked(root, &["config", "set", "consent_timeout_secs", "5"])?;
    checked(root, &["visibility", "open"])?;
    let (accepted, ready) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let first = send_file(port, "pending.txt", Some((accepted, released)));
    let first_id = wait_phase(root, Phase::AwaitingLocalConsent)?;
    checked(root, &["config", "set", "consent_timeout_secs", "1"])?;
    checked(root, &["config", "set", "transfer_timeout_secs", "1"])?;
    set_receive_directory(root, &new_directory)?;
    thread::sleep(Duration::from_millis(2200));
    assert_eq!(
        snapshot(root)?
            .active_share()
            .map(|share| (share.id().get(), share.phase())),
        Some((first_id, Phase::AwaitingLocalConsent))
    );
    checked(root, &["share", "accept", &first_id.to_string()])?;
    ready.recv_timeout(WAIT).map_err(io::Error::other)?;
    thread::sleep(Duration::from_millis(1200));
    let held = snapshot(root)?;
    let _released = release.send(());
    let sent = join_sender(first);
    assert_eq!(
        held.active_share()
            .map(|share| (share.id().get(), share.phase())),
        Some((first_id, Phase::Transferring))
    );
    sent?;
    assert_received_and_dismiss(
        root,
        first_id,
        "pending.txt",
        &old_directory,
        &new_directory,
    )?;
    assert_new_consent_timeout(root, port)
}

fn assert_new_consent_timeout(root: &Path, port: u16) -> io::Result<()> {
    let second = send_file(port, "unaccepted.txt", None);
    let second_id = wait_phase(root, Phase::AwaitingLocalConsent)?;
    let offered = Instant::now();
    let failed = wait_phase(root, Phase::Failed)?;
    let _sender_result = join_sender(second);
    assert_eq!(failed, second_id);
    assert!(offered.elapsed() < Duration::from_millis(2500));
    assert_timed_out(root)?;
    assert!(!root.join("old/unaccepted.txt").exists());
    assert!(!root.join("new/unaccepted.txt").exists());
    Ok(())
}

#[test]
fn live_preferences_pending_share_keeps_admitted_settings() -> io::Result<()> {
    isolated_worker(
        "live_preferences_pending_share_keeps_admitted_settings",
        pending_admitted_settings,
    )
}

fn wait_advertised_name(
    browser: &quickshare_network::Browser,
    port: u16,
    expected: &str,
) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(WAIT)
        .ok_or_else(|| io::Error::other("advertisement deadline overflow"))?;
    let mut last_name = None;
    while Instant::now() < deadline {
        if let Some(service) = browser
            .resolve(Duration::from_millis(100))
            .map_err(io::Error::other)?
            && service.port() == port
            && let Some(property) = service.property("n")
        {
            let endpoint =
                quickshare_sharing::EndpointInfo::decode_property(property)
                    .map_err(io::Error::other)?;
            if endpoint.device_name() == Some(expected) {
                return Ok(());
            }
            last_name = endpoint.device_name().map(String::from);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("expected advertised name {expected}, observed {last_name:?}"),
    ))
}

fn advertised_identity(root: &Path, port: u16) -> io::Result<()> {
    let dns_sd = quickshare_network::DnsSd::new().map_err(io::Error::other)?;
    let browser = dns_sd
        .browse("_FC9F5ED42C8A._tcp.local.")
        .map_err(io::Error::other)?;
    let result = (|| {
        checked(root, &["visibility", "open"])?;
        wait_advertised_name(&browser, port, "Original endpoint")?;
        checked(root, &["config", "set", "device_name", "Renamed endpoint"])?;
        wait_advertised_name(&browser, port, "Renamed endpoint")
    })();
    browser.stop().map_err(io::Error::other)?;
    dns_sd.shutdown().map_err(io::Error::other)?;
    result
}

#[test]
fn live_preferences_updates_peer_observed_name() -> io::Result<()> {
    isolated_worker(
        "live_preferences_updates_peer_observed_name",
        advertised_identity,
    )
}
