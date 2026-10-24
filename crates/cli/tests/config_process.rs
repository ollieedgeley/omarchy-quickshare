//! Offline configuration contracts through the standalone CLI.
#![cfg(test)]
#![forbid(unsafe_code)]
#![expect(
    clippy::panic_in_result_fn,
    clippy::let_underscore_untyped,
    reason = "Propagate setup errors and assert command results"
)]

use core::sync::atomic::{AtomicUsize, Ordering};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};

use clap as _;
use omarchy_quickshare_cli as _;
use quickshare_config as _;
use quickshare_control as _;
use quickshare_sharing as _;

static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

fn fixture_root() -> PathBuf {
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    env::temp_dir().join(format!(
        "omarchy-quickshare-cli-config-{}-{sequence}",
        process::id()
    ))
}

fn command(root: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_omarchy-quickshare"));
    let _ = command
        .args(arguments)
        .env("HOME", root)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("XDG_DOWNLOAD_DIR");
    command
}

fn run(root: &Path, arguments: &[&str]) -> io::Result<Output> {
    command(root, arguments).output()
}
#[test]
fn missing_config_uses_documented_defaults() {
    let root = fixture_root();
    let created = fs::create_dir_all(&root);
    assert!(created.is_ok(), "fixture root");
    let output_result = run(&root, &["config", "show"]);
    assert!(output_result.is_ok(), "config inspect");
    let Ok(output) = output_result else {
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
    assert!(body.contains("consent_timeout_secs = 300"));
    assert!(body.contains("transfer_timeout_secs = 120"));
    let cleaned = fs::remove_dir_all(&root);
    assert!(cleaned.is_ok(), "cleanup");
}

#[test]
fn missing_config_uses_xdg_download_dir_when_set() {
    let root = fixture_root();
    let created = fs::create_dir_all(&root);
    assert!(created.is_ok(), "fixture root");

    let mut xdg = command(&root, &["config", "show"]);
    let _ = xdg.env("XDG_DOWNLOAD_DIR", "/cases/received");
    let xdg_output_result = xdg.output();
    assert!(xdg_output_result.is_ok(), "xdg config inspect");
    let Ok(xdg_output) = xdg_output_result else {
        return;
    };
    assert!(xdg_output.status.success(), "xdg config inspect failed");
    let xdg_body = String::from_utf8_lossy(&xdg_output.stdout);
    assert!(xdg_body.contains(
        "receive_directory = \"/cases/received/omarchy-quickshare\""
    ));

    let mut empty = command(&root, &["config", "show"]);
    let _ = empty.env("XDG_DOWNLOAD_DIR", "");
    let empty_output_result = empty.output();
    assert!(empty_output_result.is_ok(), "empty xdg config inspect");
    let Ok(empty_output) = empty_output_result else {
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
        run(&root, &["config", "set", "receive_directory", receive_path]);
    assert!(matches!(updated, Ok(output) if output.status.success()));
    let device_name =
        run(&root, &["config", "set", "device_name", "omarchy-macbook"]);
    assert!(matches!(device_name, Ok(output) if output.status.success()));
    let inspect_result = run(&root, &["config", "show"]);
    assert!(inspect_result.is_ok(), "config inspect");
    let Ok(inspect) = inspect_result else {
        return;
    };
    let body = String::from_utf8_lossy(&inspect.stdout);
    assert!(body.contains(&receive.display().to_string()));
    assert!(body.contains("device_name = \"omarchy-macbook\""));
    let unknown_result = run(&root, &["config", "set", "not_a_key", "1"]);
    assert!(unknown_result.is_ok(), "unknown key");
    let Ok(unknown) = unknown_result else {
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
    let invalid_result = run(&root, &["config", "show"]);
    assert!(invalid_result.is_ok(), "invalid config");
    let Ok(invalid) = invalid_result else {
        return;
    };
    assert!(!invalid.status.success());
    let cleaned = fs::remove_dir_all(&root);
    assert!(cleaned.is_ok(), "cleanup");
}

#[test]
fn live_preferences_preserves_document_settings() -> io::Result<()> {
    let root = fixture_root();
    let path = root.join("config/omarchy-quickshare/config.toml");
    fs::create_dir_all(
        path.parent()
            .ok_or_else(|| io::Error::other("config parent"))?,
    )?;
    fs::write(
        &path,
        "# Keep this user's explanation\nreceive_directory = \"/cases/inbox\"\n\
         discovery_timeout_secs = 27\nvisibility_timeout_secs = 301\n\
         transfer_timeout_secs = 121\npinned_peer_id = \"peer-7\"\n",
    )?;
    let updated = run(&root, &["config", "set", "device_name", "new-name"])?;
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let saved = fs::read_to_string(&path)?;
    assert!(saved.contains("# Keep this user's explanation"));
    let inspected = run(&root, &["config", "show"])?;
    assert!(inspected.status.success());
    let body = String::from_utf8_lossy(&inspected.stdout);
    assert!(body.contains("receive_directory = \"/cases/inbox\""));
    assert!(body.contains("discovery_timeout_secs = 27"));
    assert!(body.contains("consent_timeout_secs = 301"));
    assert!(body.contains("transfer_timeout_secs = 121"));
    assert!(body.contains("pinned_peer_id = \"peer-7\""));
    assert!(body.contains("device_name = \"new-name\""));
    assert!(saved.contains("visibility_timeout_secs = 301"));
    fs::remove_dir_all(root)
}

#[test]
fn live_preferences_rejects_invalid_document_then_recovers() -> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    let path = directory.join("config.toml");
    let invalid = "# Unfinished hand edit\ndevice_name = \"unfinished\n";
    fs::write(&path, invalid)?;
    let refused = run(&root, &["config", "set", "pinned_peer_id", "peer-9"])?;
    assert!(!refused.status.success());
    assert_eq!(fs::read_to_string(&path)?, invalid);
    fs::write(
        &path,
        "device_name = \"desk\\u0020top\" # Keep inline comment\n",
    )?;
    let updated =
        run(&root, &["config", "set", "pinned_peer_id", "  peer-9  "])?;
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let saved = fs::read_to_string(&path)?;
    assert!(
        saved
            .contains("device_name = \"desk\\u0020top\" # Keep inline comment")
    );
    let inspected = run(&root, &["config", "show"])?;
    assert!(inspected.status.success());
    let body = String::from_utf8_lossy(&inspected.stdout);
    assert!(body.contains("device_name = \"desk top\""));
    assert!(body.contains("pinned_peer_id = \"peer-9\""));
    fs::remove_dir_all(root)
}

#[test]
fn live_preferences_concurrent_offline_patches_preserve_both_keys()
-> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    let path = directory.join("config.toml");
    fs::write(
        &path,
        "# Both independent changes must survive\ntransfer_timeout_secs = 91\n",
    )?;
    let name =
        command(&root, &["config", "set", "device_name", "concurrent-name"])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;
    let pin = command(
        &root,
        &["config", "set", "pinned_peer_id", "concurrent-peer"],
    )
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()?;
    let name_result = name.wait_with_output()?;
    let pin_result = pin.wait_with_output()?;
    assert!(
        name_result.status.success(),
        "{}",
        String::from_utf8_lossy(&name_result.stderr)
    );
    assert!(
        pin_result.status.success(),
        "{}",
        String::from_utf8_lossy(&pin_result.stderr)
    );
    let inspected = run(&root, &["config", "show"])?;
    assert!(inspected.status.success());
    let body = String::from_utf8_lossy(&inspected.stdout);
    assert!(body.contains("device_name = \"concurrent-name\""));
    assert!(body.contains("pinned_peer_id = \"concurrent-peer\""));
    assert!(body.contains("transfer_timeout_secs = 91"));
    assert!(
        fs::read_to_string(path)?
            .contains("# Both independent changes must survive")
    );
    fs::remove_dir_all(root)
}

#[test]
fn live_preferences_rejects_empty_receive_directory() -> io::Result<()> {
    let root = fixture_root();
    let directory = root.join("config/omarchy-quickshare");
    fs::create_dir_all(&directory)?;
    let path = directory.join("config.toml");
    let original =
        "# Keep destination\nreceive_directory = \"relative-inbox\"\n";
    fs::write(&path, original)?;
    let refused = run(&root, &["config", "set", "receive_directory", ""])?;
    assert!(!refused.status.success());
    assert_eq!(fs::read_to_string(&path)?, original);
    let inspected = run(&root, &["config", "show"])?;
    assert!(
        inspected.status.success(),
        "nonempty relative paths remain valid"
    );
    fs::write(&path, "receive_directory = \"\"\n")?;
    let invalid = run(&root, &["config", "show"])?;
    assert!(!invalid.status.success());
    fs::remove_dir_all(root)
}
