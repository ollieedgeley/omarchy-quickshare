use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process;

use quickshare_control::request::Envelope as RequestEnvelope;

use super::{request, request_for_peer, run};

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
    let envelope = request(&file_uri(&path), Some(&root), "cli-test-request")
        .expect("request");
    assert_eq!(
        envelope,
        RequestEnvelope::submit_file(&path, "cli-test-request")
    );
    drop(fs::remove_dir_all(&root));
}

#[test]
fn encoded_folder_uri_with_spaces_submits_as_folder() {
    let root = fixture("folder-spaces");
    let path = root.join("Quick Share Folder");
    fs::create_dir_all(&path).expect("folder");
    fs::write(path.join("note.txt"), b"hi").expect("file");
    let envelope = request(&file_uri(&path), Some(&root), "cli-test-request")
        .expect("request");
    assert_eq!(
        envelope,
        RequestEnvelope::submit_file(&path, "cli-test-request")
    );
    drop(fs::remove_dir_all(&root));
}

#[test]
fn localhost_file_uri_is_local() {
    let root = fixture("localhost");
    let path = root.join("note.txt");
    fs::write(&path, b"hi").expect("file");
    let uri = format!("file://localhost{}", path.display());
    let envelope =
        request(&uri, Some(&root), "cli-test-request").expect("request");
    assert_eq!(
        envelope,
        RequestEnvelope::submit_file(&path, "cli-test-request")
    );
    drop(fs::remove_dir_all(&root));
}

#[test]
fn single_uri_list_line_submits_as_file() {
    let root = fixture("uri-list");
    let path = root.join("note.txt");
    fs::write(&path, b"hi").expect("file");
    let uri = format!("# comment\n{}\n", file_uri(&path));
    let envelope =
        request(&uri, Some(&root), "cli-test-request").expect("request");
    assert_eq!(
        envelope,
        RequestEnvelope::submit_file(&path, "cli-test-request")
    );
    drop(fs::remove_dir_all(&root));
}

#[test]
fn remote_file_uri_is_rejected() {
    let error =
        request("file://example.com/tmp/note.txt", None, "cli-test-request")
            .expect_err("remote host");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn malformed_percent_escape_is_rejected() {
    let error =
        request("file:///tmp/Quick%2Share.apk", None, "cli-test-request")
            .expect_err("malformed");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn nul_in_file_uri_is_rejected() {
    let error =
        request("file:///tmp/Quick%00Share.apk", None, "cli-test-request")
            .expect_err("nul");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn multi_entry_uri_list_is_rejected() {
    let error =
        request("file:///tmp/a\nfile:///tmp/b", None, "cli-test-request")
            .expect_err("multi");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn missing_file_uri_does_not_fall_back_to_text() {
    let error =
        request("file:///no-such-quickshare-file", None, "cli-test-request")
            .expect_err("missing");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn plain_text_and_urls_keep_existing_classification() {
    let cwd = PathBuf::from("/tmp");
    assert_eq!(
        request("hello", Some(&cwd), "cli-test-request").expect("text"),
        RequestEnvelope::submit_text("hello", "cli-test-request")
    );
    assert_eq!(
        request("https://example.com", Some(&cwd), "cli-test-request")
            .expect("url"),
        RequestEnvelope::submit_url("https://example.com", "cli-test-request")
    );
}

#[test]
fn peer_targeted_text_keeps_content_and_recipient_together() {
    let cwd = PathBuf::from("/tmp");
    assert_eq!(
        request_for_peer("hello", Some(&cwd), "pixel-8", "cli-test-request")
            .expect("targeted text"),
        RequestEnvelope::submit_text_to_peer(
            "hello",
            "pixel-8",
            "cli-test-request"
        )
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
fn clipboard_file_uri_is_revalidated_after_capture() {
    let root = fixture("clipboard-revalidation");
    let path = root.join("captured A.txt");
    fs::write(&path, b"captured file").expect("file");
    let captured = format!("# captured\n{}\r\n", file_uri(&path));
    assert_eq!(
        request_for_peer(&captured, None, "pixel-8", "cli-test-request")
            .expect("captured file"),
        RequestEnvelope::submit_file_to_peer(
            &path,
            "pixel-8",
            "cli-test-request"
        )
    );
    fs::remove_file(&path).expect("remove captured file");
    assert_eq!(
        request_for_peer(&captured, None, "pixel-8", "cli-test-request")
            .expect_err("removed source")
            .kind(),
        std::io::ErrorKind::InvalidInput
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn clipboard_path_text_does_not_infer_files_but_manual_send_does() {
    let root = fixture("clipboard-path-text");
    let content = "captured A.txt";
    let path = root.join(content);
    fs::write(&path, b"not clipboard text").expect("file");
    assert_eq!(
        request_for_peer(content, None, "pixel-8", "cli-test-request")
            .expect("clipboard text"),
        RequestEnvelope::submit_text_to_peer(
            content,
            "pixel-8",
            "cli-test-request"
        )
    );
    assert_eq!(
        request_for_peer(content, Some(&root), "pixel-8", "cli-test-request")
            .expect("manual file"),
        RequestEnvelope::submit_file_to_peer(
            &path,
            "pixel-8",
            "cli-test-request"
        )
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn clipboard_literal_text_and_url_preserve_original_bytes() {
    let text = "  ./captured A.txt\r\nhttps://example.test/\u{732b}\n";
    let url = "https://example.test/\u{732b}?q=%20+value\n";
    assert_eq!(
        request(text, None, "cli-test-request").expect("literal text"),
        RequestEnvelope::submit_text(text, "cli-test-request")
    );
    assert_eq!(
        request(url, None, "cli-test-request").expect("literal URL"),
        RequestEnvelope::submit_url(url, "cli-test-request")
    );
}

#[test]
fn uncertain_submission_retains_id_without_retry() {
    use quickshare_control::codec::read_request;
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;

    let root = fixture("uncertain-submission");
    let socket = root.join("control.sock");
    let listener = UnixListener::bind(&socket).expect("listener");
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("submission connection");
        let envelope =
            read_request(&mut BufReader::new(stream)).expect("submission");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        (envelope, listener)
    });
    let error = run(
        &[String::from("send"), String::from("hello")],
        &root,
        &socket,
        &mut Vec::new(),
    )
    .expect_err("lost acknowledgement");
    let (envelope, returned_listener) = server.join().expect("server");
    let request_id = envelope.request_id().expect("retained submission ID");
    assert_eq!(request_id.len(), 32);
    assert!(error.to_string().contains(&format!(
        "omarchy-quickshare operation {request_id} --json"
    )));
    assert_eq!(
        returned_listener
            .accept()
            .expect_err("no mutation retry")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
    drop(returned_listener);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn operation_unknown_json_does_not_claim_nonacceptance() {
    use quickshare_control::codec::{
        read_request, read_response, write_response,
    };
    use quickshare_control::response::{Envelope, Response};
    use std::io::BufReader;
    use std::os::unix::net::UnixListener;

    let root = fixture("unknown-operation");
    let socket = root.join("control.sock");
    let listener = UnixListener::bind(&socket).expect("listener");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("query connection");
        let envelope =
            read_request(&mut BufReader::new(&mut stream)).expect("query");
        assert_eq!(envelope, RequestEnvelope::operation("retained-request"));
        assert_eq!(envelope.request_id(), None);
        write_response(&mut stream, &Envelope::not_found())
            .expect("unknown response");
    });
    let mut output = Vec::new();
    let error = run(
        &[
            String::from("operation"),
            String::from("retained-request"),
            String::from("--json"),
        ],
        &root,
        &socket,
        &mut output,
    )
    .expect_err("unknown outcome");
    server.join().expect("server");
    assert!(matches!(
        read_response(&mut output.as_slice())
            .expect("JSON")
            .response(),
        Response::NotFound
    ));
    assert!(error.to_string().contains("unknown or expired"));
    assert!(
        error
            .to_string()
            .contains("does not prove it was never accepted")
    );
    fs::remove_dir_all(root).expect("cleanup");
}
