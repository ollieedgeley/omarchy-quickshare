#[path = "worker_preferences.rs"]
mod worker_preferences;

extern crate alloc;

use super::{
    Fixture, RETRY_DELAY, START_TIMEOUT, command, fixture_root, snapshot,
};
use alloc::collections::BTreeMap;
use quickshare_control::codec::{read_response, write_request};
use quickshare_control::request::Envelope as RequestEnvelope;
use quickshare_control::response::{PreferenceStatus, Response};
use quickshare_sharing::PeerSnapshot;
use std::fs;
use std::io::{self, BufReader};
use std::os::unix::net::UnixStream;
use std::panic::{catch_unwind, resume_unwind};
use std::path::Path;
use std::thread;
use std::time::Instant;

fn preferences(root: &Path) -> io::Result<PreferenceStatus> {
    let output = command(root, &["status", "--json"]).output()?;
    if !output.status.success() {
        return Err(io::Error::other("JSON status failed"));
    }
    let envelope = read_response(&mut output.stdout.as_slice())?;
    if let Response::Snapshot { preferences, .. } = envelope.response() {
        Ok(preferences.clone())
    } else {
        Err(io::Error::other("expected preference snapshot"))
    }
}

#[test]
fn live_preferences_status_keeps_applied_values_after_rejection()
-> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    let applied_directory = root.join("inbox");
    let occupied = root.join("occupied");
    fs::create_dir_all(&directory)?;
    fs::create_dir_all(&applied_directory)?;
    fs::write(&occupied, "not a directory")?;
    let path = directory.join("config.toml");
    fs::write(
        &path,
        format!(
            "receive_directory = {applied_directory:?}\n\
             pinned_peer_id = \"absent-peer\"\n"
        ),
    )?;
    let fixture = Fixture::start(root.clone())?;
    let result = catch_unwind(|| -> io::Result<()> {
        let initial = preferences(&root)?;
        assert_eq!(
            initial
                .applied
                .as_ref()
                .map(|value| &value.receive_directory),
            Some(&applied_directory)
        );
        assert_rejected_directory(&root, &occupied, &initial)?;
        assert_plain_applied_preferences(&root, &applied_directory)?;
        assert_invalid_document(&root, &path, &initial)?;
        assert_plain_applied_preferences(&root, &applied_directory)
    });
    fixture.stop()?;
    match result {
        Ok(result) => result,
        Err(panic) => resume_unwind(panic),
    }
}

fn assert_rejected_directory(
    root: &Path,
    occupied: &Path,
    initial: &PreferenceStatus,
) -> io::Result<()> {
    let changed = command(
        root,
        &[
            "config",
            "set",
            "receive_directory",
            occupied.to_str().ok_or_else(|| io::Error::other("path"))?,
        ],
    )
    .output()?;
    assert!(
        !changed.status.success(),
        "activation failure must be reported"
    );
    let rejected = preferences(root)?;
    assert_eq!(
        rejected
            .saved
            .as_ref()
            .map(|value| value.receive_directory.as_path()),
        Some(occupied)
    );
    assert_eq!(rejected.applied, initial.applied);
    assert!(rejected.error.is_some());
    Ok(())
}

fn assert_invalid_document(
    root: &Path,
    path: &Path,
    initial: &PreferenceStatus,
) -> io::Result<()> {
    fs::write(path, "device_name = \"unfinished\n")?;
    let deadline = Instant::now()
        .checked_add(START_TIMEOUT)
        .ok_or_else(|| io::Error::other("startup deadline overflowed"))?;
    let invalid = loop {
        let observed = preferences(root)?;
        if observed.saved.is_none() || Instant::now() >= deadline {
            break observed;
        }
        thread::sleep(RETRY_DELAY);
    };
    assert!(invalid.saved.is_none(), "invalid document must be observed");
    assert_eq!(invalid.applied, initial.applied);
    assert!(invalid.error.is_some());
    Ok(())
}

fn assert_plain_applied_preferences(
    root: &Path,
    applied_directory: &Path,
) -> io::Result<()> {
    let plain = command(root, &["status"]).output()?;
    assert!(plain.status.success(), "plain status must remain usable");
    let body = String::from_utf8_lossy(&plain.stdout);
    let fields: BTreeMap<_, _> = body
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    assert_eq!(
        fields.get("receive_directory").copied(),
        applied_directory.to_str()
    );
    assert_eq!(fields.get("pinned_peer").copied(), Some("absent-peer"));
    assert!(fields.contains_key("preferences_error"));
    Ok(())
}

#[test]
fn live_preferences_external_rename_applies_preferred_peer() -> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("config.toml"),
        "pinned_peer_id = \"old-peer\"\n",
    )?;
    let fixture = Fixture::start(root.clone())?;
    let observed =
        command(&root, &["simulate", "peer-seen", "new-peer", "New peer"])
            .env("OMARCHY_QUICKSHARE_ALLOW_SIMULATION", "1")
            .output()?;
    assert!(observed.status.success(), "observe peer");
    assert!(
        snapshot(&root)?
            .peers()
            .iter()
            .all(|peer| !peer.is_pinned())
    );
    fs::write(
        directory.join("editor-save"),
        "pinned_peer_id = \"new-peer\"\n",
    )?;
    fs::rename(directory.join("editor-save"), directory.join("config.toml"))?;
    let deadline = Instant::now()
        .checked_add(START_TIMEOUT)
        .ok_or_else(|| io::Error::other("startup deadline overflowed"))?;
    let applied = loop {
        if snapshot(&root)?.peers().iter().any(PeerSnapshot::is_pinned) {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        thread::sleep(RETRY_DELAY);
    };
    fixture.stop()?;
    assert!(
        applied,
        concat!(
            "external rename-save must change the running preferred peer ",
            "within one second"
        )
    );
    Ok(())
}

#[test]
fn live_preferences_failed_pin_preserves_running_preference() -> io::Result<()>
{
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    let path = directory.join("config.toml");
    fs::write(&path, "pinned_peer_id = \"pixel-8\"\n")?;
    let mut fixture = Fixture::start(root.clone())?;
    fs::remove_file(&path)?;
    fs::create_dir_all(&path)?;
    let result = command(&root, &["peer", "pin", "galaxy-tab"]).output()?;
    assert!(
        !result.status.success(),
        "a failed write must not report success"
    );
    let observed = snapshot(&root);
    if fixture.child.try_wait()?.is_none() {
        fixture.stop()?;
    } else {
        fs::remove_dir_all(root)?;
    }
    let state = observed?;
    assert!(
        state
            .peers()
            .iter()
            .any(|peer| peer.id() == "pixel-8" && peer.is_pinned())
    );
    assert!(
        state
            .peers()
            .iter()
            .all(|peer| peer.id() != "galaxy-tab" || !peer.is_pinned())
    );
    Ok(())
}

fn wait_for_config_lock(process_id: u32) -> io::Result<()> {
    let process_id = process_id.to_string();
    let deadline = Instant::now()
        .checked_add(START_TIMEOUT)
        .ok_or_else(|| io::Error::other("startup deadline overflowed"))?;
    loop {
        if fs::read_to_string("/proc/locks")?.lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields.contains(&"->")
                && fields.windows(2).any(|pair| pair == ["WRITE", &process_id])
        }) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "daemon did not wait for config lock",
            ));
        }
        thread::sleep(RETRY_DELAY);
    }
}

#[test]
fn live_preferences_failed_patch_applies_external_edit() -> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    let path = directory.join("config.toml");
    fs::write(&path, "device_name = \"Before edit\"\n")?;
    let fixture = Fixture::start(root.clone())?;
    let result = catch_unwind(|| -> io::Result<()> {
        let stream = patch_during_external_edit(
            &root,
            &directory,
            &path,
            fixture.child.id(),
        )?;
        let response = read_response(&mut BufReader::new(stream))?;
        let Response::Preferences {
            preferences: rejected,
        } = response.response()
        else {
            return Err(io::Error::other(
                "expected rejected patch preferences",
            ));
        };
        assert!(
            rejected.error.is_some(),
            "invalid patch must report failure"
        );
        assert_external_edit_applied(&root, &path)
    });
    fixture.stop()?;
    match result {
        Ok(outcome) => outcome,
        Err(panic) => resume_unwind(panic),
    }
}

fn patch_during_external_edit(
    root: &Path,
    directory: &Path,
    path: &Path,
    process_id: u32,
) -> io::Result<UnixStream> {
    let lock = fs::File::create(directory.join(".config.lock"))?;
    lock.lock()?;
    let mut stream =
        UnixStream::connect(root.join("omarchy-quickshare/control.sock"))?;
    stream.set_read_timeout(Some(START_TIMEOUT))?;
    write_request(
        &mut stream,
        &RequestEnvelope::patch_preferences("transfer_timeout_secs", "invalid"),
    )?;
    wait_for_config_lock(process_id)?;
    fs::write(path, "device_name = \"External edit\"\n")?;
    drop(lock);
    Ok(stream)
}

fn assert_external_edit_applied(root: &Path, path: &Path) -> io::Result<()> {
    let deadline = Instant::now()
        .checked_add(START_TIMEOUT)
        .ok_or_else(|| io::Error::other("startup deadline overflowed"))?;
    loop {
        let observed = preferences(root)?;
        if observed
            .applied
            .as_ref()
            .and_then(|values| values.device_name.as_deref())
            == Some("External edit")
        {
            assert!(!observed.pending);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "external edit was lost after patch rejection: {observed:?}"
        );
        thread::sleep(RETRY_DELAY);
    }
    assert_eq!(
        fs::read_to_string(path)?,
        "device_name = \"External edit\"\n"
    );
    Ok(())
}
