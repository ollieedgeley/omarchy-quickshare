use std::fs;
use std::path::PathBuf;
use std::process;

use quickshare_control::request::Envelope as RequestEnvelope;

use super::request;

fn fixture(name: &str) -> PathBuf {
    let root = std::env::temp_dir()
        .join(format!("omarchy-quickshare-cli-{}-{name}", process::id()));
    drop(fs::remove_dir_all(&root));
    fs::create_dir_all(&root).expect("test root");
    root
}

fn file_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display()).replace(' ', "%20")
}

#[test]
fn encoded_file_uri_with_spaces_submits_as_file() {
    let root = fixture("file-spaces");
    let path = root.join("Quick Share.apk");
    fs::write(&path, b"apk").expect("file");
    let envelope = request(&[file_uri(&path)], &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn encoded_folder_uri_with_spaces_submits_as_folder() {
    let root = fixture("folder-spaces");
    let path = root.join("Quick Share Folder");
    fs::create_dir_all(&path).expect("folder");
    fs::write(path.join("note.txt"), b"hi").expect("file");
    let envelope = request(&[file_uri(&path)], &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn localhost_file_uri_is_local() {
    let root = fixture("localhost");
    let path = root.join("note.txt");
    fs::write(&path, b"hi").expect("file");
    let uri = format!("file://localhost{}", path.display());
    let envelope = request(&[uri], &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn single_uri_list_line_submits_as_file() {
    let root = fixture("uri-list");
    let path = root.join("note.txt");
    fs::write(&path, b"hi").expect("file");
    let uri = format!("# comment\n{}\n", file_uri(&path));
    let envelope = request(&[uri], &root).expect("request");
    assert_eq!(envelope, RequestEnvelope::submit_file(&path));
    drop(fs::remove_dir_all(&root));
}

#[test]
fn remote_file_uri_is_rejected() {
    let error = request(
        &[String::from("file://example.com/tmp/note.txt")],
        &PathBuf::from("/tmp"),
    )
    .expect_err("remote host");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("not local"));
}

#[test]
fn malformed_percent_escape_is_rejected() {
    let error = request(
        &[String::from("file:///tmp/Quick%2Share.apk")],
        &PathBuf::from("/tmp"),
    )
    .expect_err("malformed");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("malformed"));
}

#[test]
fn nul_in_file_uri_is_rejected() {
    let error = request(
        &[String::from("file:///tmp/Quick%00Share.apk")],
        &PathBuf::from("/tmp"),
    )
    .expect_err("nul");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("NUL"));
}

#[test]
fn multi_entry_uri_list_is_rejected() {
    let error = request(
        &[String::from("file:///tmp/a\nfile:///tmp/b")],
        &PathBuf::from("/tmp"),
    )
    .expect_err("multi");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("multiple entries"));
}

#[test]
fn missing_file_uri_does_not_fall_back_to_text() {
    let error = request(
        &[String::from("file:///no-such-quickshare-file")],
        &PathBuf::from("/tmp"),
    )
    .expect_err("missing");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("does not exist"));
}

#[test]
fn extra_argument_is_rejected() {
    let error = request(
        &[String::from("hello"), String::from("world")],
        &PathBuf::from("/tmp"),
    )
    .expect_err("usage");
    assert!(error.to_string().contains("usage:"));
}

#[test]
fn plain_text_and_urls_keep_existing_classification() {
    let cwd = PathBuf::from("/tmp");
    assert_eq!(
        request(&[String::from("hello")], &cwd).expect("text"),
        RequestEnvelope::submit_text("hello")
    );
    assert_eq!(
        request(&[String::from("https://example.com")], &cwd).expect("url"),
        RequestEnvelope::submit_url("https://example.com")
    );
}
