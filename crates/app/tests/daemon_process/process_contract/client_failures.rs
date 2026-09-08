use std::fs;
use std::io::{self, BufReader, Write as _};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::thread;

use quickshare_control::codec::{read_request, write_response};
use quickshare_control::response::{Envelope, ErrorCode};

use super::{
    DAEMON_BINARY, process_command, run_command, runtime_fixture_path,
};
#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Process contracts propagate setup errors and assert command results"
)]
fn protocol_introspection_needs_no_runtime_directory() -> io::Result<()> {
    let root = runtime_fixture_path();
    let expected = format!("{}\n", quickshare_control::PROTOCOL_VERSION);
    for (binary, arguments) in [
        (PathBuf::from(DAEMON_BINARY), vec!["--protocol-version"]),
        (
            Path::new(DAEMON_BINARY).with_file_name("omarchy-quickshare"),
            vec!["protocol-version"],
        ),
    ] {
        let output = process_command(&binary, &root, &arguments)
            .env_remove("XDG_RUNTIME_DIR")
            .output()?;
        assert!(output.status.success());
        assert_eq!(output.stdout, expected.as_bytes());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Process contracts propagate setup errors and assert command results"
)]
fn status_json_rejects_invalid_responses() -> io::Result<()> {
    let mut wrong_reply = Vec::new();
    write_response(&mut wrong_reply, &Envelope::applied())?;
    let mut control_error = Vec::new();
    write_response(
        &mut control_error,
        &Envelope::error(ErrorCode::InvalidRequest),
    )?;
    let incompatible = format!(
        "{{\"response\":{{\"type\":\"ready\"}},\"version\":{}}}\n",
        quickshare_control::PROTOCOL_VERSION + 1
    );
    for response in [
        wrong_reply,
        control_error,
        b"not json\n".to_vec(),
        incompatible.into_bytes(),
    ] {
        let root = runtime_fixture_path();
        let socket_directory = root.join("omarchy-quickshare");
        fs::create_dir_all(&socket_directory)?;
        let listener =
            UnixListener::bind(socket_directory.join("control.sock"))?;
        let worker = thread::spawn(move || -> io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let _request =
                read_request(&mut BufReader::new(stream.try_clone()?))?;
            stream.write_all(&response)
        });
        let output = run_command(&root, &["status", "--json"])?;
        worker
            .join()
            .map_err(|_panic| io::Error::other("control fake panicked"))??;
        fs::remove_dir_all(root)?;
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Process contracts propagate setup errors and assert command results"
)]
fn status_json_fails_when_daemon_is_absent() -> io::Result<()> {
    let root = runtime_fixture_path();
    fs::create_dir_all(&root)?;
    let output = run_command(&root, &["status", "--json"])?;
    fs::remove_dir_all(root)?;
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    Ok(())
}
