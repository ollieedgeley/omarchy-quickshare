use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process;

use quickshare_control::request::Envelope as RequestEnvelope;
use tracing_subscriber::EnvFilter;

use super::{Command, LogLevel, env_filter, parse, request, run};

fn fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir()
        .join(format!("omarchy-quickshare-cli-{}-{name}", process::id()));
    drop(fs::remove_dir_all(&root));
    fs::create_dir_all(&root).expect("test root");
    root
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", path.display()).replace(' ', "%20")
}

#[test]
fn encoded_file_uri_with_spaces_submits_as_file() {
    let root = fixture("file-spaces");
    let path = root.join("Quick Share.apk");
    fs::write(&path, b"apk").expect("file");
    let envelope = request(&file_uri(&path), &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn encoded_folder_uri_with_spaces_submits_as_folder() {
    let root = fixture("folder-spaces");
    let path = root.join("Quick Share Folder");
    fs::create_dir_all(&path).expect("folder");
    fs::write(path.join("note.txt"), b"hi").expect("file");
    let envelope = request(&file_uri(&path), &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn localhost_file_uri_is_local() {
    let root = fixture("localhost");
    let path = root.join("note.txt");
    fs::write(&path, b"hi").expect("file");
    let uri = format!("file://localhost{}", path.display());
    let envelope = request(&uri, &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn single_uri_list_line_submits_as_file() {
    let root = fixture("uri-list");
    let path = root.join("note.txt");
    fs::write(&path, b"hi").expect("file");
    let uri = format!("# comment\n{}\n", file_uri(&path));
    let envelope = request(&uri, &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn remote_file_uri_is_rejected() {
    let error = request("file://example.com/tmp/note.txt", Path::new("/tmp"))
        .expect_err("remote host");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("not local"));
}

#[test]
fn malformed_percent_escape_is_rejected() {
    let error = request("file:///tmp/Quick%2Share.apk", Path::new("/tmp"))
        .expect_err("malformed");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("malformed"));
}

#[test]
fn nul_in_file_uri_is_rejected() {
    let error = request("file:///tmp/Quick%00Share.apk", Path::new("/tmp"))
        .expect_err("nul");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("NUL"));
}

#[test]
fn multi_entry_uri_list_is_rejected() {
    let error = request("file:///tmp/a\nfile:///tmp/b", Path::new("/tmp"))
        .expect_err("multi");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("multiple entries"));
}

#[test]
fn missing_file_uri_does_not_fall_back_to_text() {
    let error = request("file:///no-such-quickshare-file", Path::new("/tmp"))
        .expect_err("missing");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("does not exist"));
}

#[test]
fn plain_text_and_urls_keep_existing_classification() {
    let cwd = PathBuf::from("/tmp");
    assert_eq!(
        request("hello", &cwd).expect("text"),
        RequestEnvelope::submit_text("hello")
    );
    assert_eq!(
        request("https://example.com", &cwd).expect("url"),
        RequestEnvelope::submit_url("https://example.com")
    );
}

#[test]
fn help_does_not_become_submit_text() {
    let mut output = Vec::new();
    let result = run(
        &[String::from("--help")],
        Path::new("."),
        Path::new("missing-control.sock"),
        &mut output,
    );
    assert!(result.is_ok(), "help failed: {result:?}");
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("Usage:"));
    assert!(text.contains("send"));
}

#[test]
fn version_does_not_contact_the_daemon() {
    let mut output = Vec::new();
    let result = run(
        &[String::from("--version")],
        Path::new("."),
        Path::new("missing-control.sock"),
        &mut output,
    );
    assert!(result.is_ok(), "version failed: {result:?}");
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn missing_subcommand_does_not_submit_text() {
    let mut output = Vec::new();
    let result = run(
        &[],
        Path::new("."),
        Path::new("missing-control.sock"),
        &mut output,
    );
    assert!(result.is_err(), "empty argv submitted content");
}

#[test]
fn unknown_option_does_not_submit_text() {
    let mut output = Vec::new();
    let result = run(
        &[String::from("--nope")],
        Path::new("."),
        Path::new("missing-control.sock"),
        &mut output,
    );
    assert!(result.is_err(), "unknown option submitted content");
}

#[test]
fn send_and_control_commands_map_to_the_command_tree() {
    let send = parse(["send", "hello"]).expect("send");
    assert!(matches!(send.into_command(), Command::Send { .. }));
    let health = parse(["health"]).expect("health");
    assert!(matches!(health.into_command(), Command::Health));
    let status = parse(["status", "--json"]).expect("status");
    assert!(matches!(
        status.into_command(),
        Command::Status { json: true }
    ));
    let share = parse(["share", "select", "1", "pixel-8"]).expect("share");
    assert!(matches!(share.into_command(), Command::Share { .. }));
}

#[test]
fn daemon_log_level_defaults_to_info() {
    let cli = parse(["daemon"]).expect("daemon");
    assert!(
        matches!(
            cli.into_command(),
            Command::Daemon {
                simulate: false,
                log_level: LogLevel::Info,
            }
        ),
        "expected default daemon logging"
    );
    let debug = parse(["daemon", "--log-level", "debug"]).expect("debug");
    assert!(
        matches!(
            debug.into_command(),
            Command::Daemon {
                log_level: LogLevel::Debug,
                ..
            }
        ),
        "expected debug log level"
    );
}

#[test]
fn env_filter_uses_cli_level_when_rust_log_is_unset() {
    if std::env::var_os("RUST_LOG").is_some() {
        return;
    }
    let filter = env_filter(LogLevel::Warn);
    assert_eq!(filter.to_string(), EnvFilter::new("warn").to_string());
}
