#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::env;
    use std::fs;
    use std::io::{self, BufReader};
    use std::io::{BufRead as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::process;
    use std::thread::{self, JoinHandle};

    use omarchy_quickshare::daemon::Daemon;

    const REQUEST_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v2/submit-text-request.jsonl"
    );
    const RESPONSE_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v2/submit-text-queued-response.jsonl"
    );
    const URL_REQUEST_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v2/submit-url-request.jsonl"
    );
    const STATUS_REQUEST_FIXTURE: &str =
        include_str!("../../../../fixtures/control/v2/status-request.jsonl");
    const STATUS_RESPONSE_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v2/status-ready-response.jsonl"
    );
    const EXPECTED_OUTPUT: &[u8] = b"Share 1 queued.\n";
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct ControlDaemonFake {
        root: PathBuf,
        socket: PathBuf,
        worker: JoinHandle<io::Result<String>>,
    }

    struct LocalEndpointFixture {
        root: PathBuf,
        socket: PathBuf,
        worker: JoinHandle<io::Result<Daemon>>,
    }

    struct FileFixture {
        path: PathBuf,
        root: PathBuf,
    }

    impl FileFixture {
        fn cleanup(self) -> io::Result<()> {
            fs::remove_dir_all(self.root)
        }

        fn create() -> io::Result<Self> {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "omarchy-quickshare-file-{}-{sequence}",
                process::id()
            ));
            fs::create_dir_all(&root)?;
            let path = root.join("photo.jpg");
            fs::write(&path, b"fixture bytes")?;
            Ok(Self { path, root })
        }
    }

    impl ControlDaemonFake {
        fn finish(self) -> io::Result<String> {
            drop(UnixStream::connect(&self.socket));
            let request = self.worker.join().map_err(|_panic| {
                io::Error::other("control fake panicked")
            })??;
            fs::remove_dir_all(self.root)?;
            Ok(request)
        }

        fn socket(&self) -> &Path {
            &self.socket
        }

        fn start() -> io::Result<Self> {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "omarchy-quickshare-control-{}-{sequence}",
                process::id()
            ));
            fs::create_dir_all(&root)?;
            let socket = root.join("control.sock");
            let listener = UnixListener::bind(&socket)?;
            let worker = thread::spawn(move || serve_once(&listener));
            Ok(Self {
                root,
                socket,
                worker,
            })
        }
    }

    impl LocalEndpointFixture {
        fn finish(self) -> io::Result<Daemon> {
            let endpoint = self.worker.join().map_err(|_panic| {
                io::Error::other("local endpoint panicked")
            })?;
            fs::remove_dir_all(self.root)?;
            endpoint
        }

        fn socket(&self) -> &Path {
            &self.socket
        }

        fn start() -> io::Result<Self> {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "omarchy-quickshare-endpoint-{}-{sequence}",
                process::id()
            ));
            fs::create_dir_all(&root)?;
            let socket = root.join("control.sock");
            let listener = UnixListener::bind(&socket)?;
            let worker = thread::spawn(move || {
                let mut endpoint = Daemon::new();
                endpoint.serve_next(&listener)?;
                io::Result::Ok(endpoint)
            });
            Ok(Self {
                root,
                socket,
                worker,
            })
        }
    }

    #[expect(
        clippy::single_call_fn,
        reason = "The fake worker names its one external protocol interaction"
    )]
    fn serve_once(listener: &UnixListener) -> io::Result<String> {
        let (mut stream, _) = listener.accept()?;
        let mut request = String::new();
        let _bytes_read =
            BufReader::new(stream.try_clone()?).read_line(&mut request)?;
        if request == STATUS_REQUEST_FIXTURE {
            stream.write_all(STATUS_RESPONSE_FIXTURE.as_bytes())?;
            return Ok(request);
        }
        if !request.is_empty() {
            stream.write_all(RESPONSE_FIXTURE.as_bytes())?;
        }
        Ok(request)
    }

    fn assert_submission(value: &str, expected_request: &str) {
        assert_submission_from(value, Path::new("."), expected_request);
    }

    fn assert_submission_from(
        value: &str,
        current_directory: &Path,
        expected_request: &str,
    ) {
        let fake_result = ControlDaemonFake::start();
        assert!(fake_result.is_ok(), "failed to start control fake");
        let Ok(fake) = fake_result else {
            return;
        };
        let mut output = Vec::new();
        let arguments = vec![String::from(value)];

        let run_result = omarchy_quickshare::run(
            &arguments,
            current_directory,
            fake.socket(),
            &mut output,
        );
        assert!(run_result.is_ok(), "submission failed: {run_result:?}");

        let request_result = fake.finish();
        assert!(
            request_result.is_ok(),
            "control fake failed: {request_result:?}",
        );
        let Ok(request) = request_result else {
            return;
        };
        assert_eq!(request, expected_request);
        assert_eq!(output, EXPECTED_OUTPUT);
    }

    #[test]
    fn bare_text_is_submitted_to_the_local_endpoint() {
        assert_submission("hello from Omarchy", REQUEST_FIXTURE);
    }

    #[test]
    fn directory_is_submitted_to_the_local_endpoint_for_archiving() {
        let fixture_result = FileFixture::create();
        assert!(fixture_result.is_ok(), "failed to create file fixture");
        let Ok(fixture) = fixture_result else {
            return;
        };
        let path = fixture.root.join(".").to_string_lossy().into_owned();
        let expected = format!(
            "{{\"request\":{{\"type\":\"submit_file\",\"path\":\"{path}\"}},\
             \"version\":2}}\n"
        );
        assert_submission_from(".", &fixture.root, &expected);
        let cleanup_result = fixture.cleanup();
        assert!(cleanup_result.is_ok(), "failed to clean file fixture");
    }

    #[test]
    fn url_is_submitted_to_the_local_endpoint_as_a_url() {
        assert_submission("https://example.com/share", URL_REQUEST_FIXTURE);
    }

    #[test]
    fn existing_file_is_submitted_to_the_local_endpoint_as_a_file() {
        let fixture_result = FileFixture::create();
        assert!(fixture_result.is_ok(), "failed to create file fixture");
        let Ok(fixture) = fixture_result else {
            return;
        };
        let path = fixture.path.to_string_lossy();
        let expected = format!(
            "{{\"request\":{{\"type\":\"submit_file\",\"path\":\"{path}\"}},\
             \"version\":2}}\n"
        );
        assert_submission_from("photo.jpg", &fixture.root, &expected);
        let cleanup_result = fixture.cleanup();
        assert!(cleanup_result.is_ok(), "failed to clean file fixture");
    }

    #[test]
    fn control_protocol_version_is_reported_without_an_endpoint() {
        let arguments = vec![String::from("--protocol-version")];
        let mut output = Vec::new();

        let result = omarchy_quickshare::run(
            &arguments,
            Path::new("."),
            Path::new("missing-control.sock"),
            &mut output,
        );

        assert!(result.is_ok(), "version query failed: {result:?}");
        assert_eq!(output, b"2\n");
    }

    #[test]
    fn runtime_status_reports_an_available_local_endpoint() {
        let fake_result = ControlDaemonFake::start();
        assert!(fake_result.is_ok(), "failed to start control fake");
        let Ok(fake) = fake_result else {
            return;
        };
        let arguments = vec![String::from("--runtime-status")];
        let mut output = Vec::new();

        let result = omarchy_quickshare::run(
            &arguments,
            Path::new("."),
            fake.socket(),
            &mut output,
        );

        assert!(result.is_ok(), "runtime query failed: {result:?}");
        assert_eq!(output, b"available\n");
        let request_result = fake.finish();
        assert!(request_result.is_ok(), "control fake failed");
        assert_eq!(request_result.unwrap_or_default(), STATUS_REQUEST_FIXTURE);
    }

    #[test]
    fn real_local_endpoint_owns_the_queued_share() {
        let fixture_result = LocalEndpointFixture::start();
        assert!(fixture_result.is_ok(), "failed to start local endpoint");
        let Ok(fixture) = fixture_result else {
            return;
        };
        let mut output = Vec::new();
        let arguments = vec![String::from("hello from Omarchy")];

        let run_result = omarchy_quickshare::run(
            &arguments,
            Path::new("."),
            fixture.socket(),
            &mut output,
        );

        assert!(run_result.is_ok(), "submission failed: {run_result:?}");
        let endpoint_result = fixture.finish();
        assert!(endpoint_result.is_ok(), "local endpoint failed");
        let Ok(endpoint) = endpoint_result else {
            return;
        };
        assert_eq!(endpoint.queued_count(), 1);
        assert_eq!(output, EXPECTED_OUTPUT);
    }

    #[test]
    fn real_local_endpoint_reports_ready_without_queueing_a_share() {
        let fixture_result = LocalEndpointFixture::start();
        assert!(fixture_result.is_ok(), "failed to start local endpoint");
        let Ok(fixture) = fixture_result else {
            return;
        };
        let mut output = Vec::new();
        let arguments = vec![String::from("--runtime-status")];

        let run_result = omarchy_quickshare::run(
            &arguments,
            Path::new("."),
            fixture.socket(),
            &mut output,
        );

        assert!(run_result.is_ok(), "runtime query failed: {run_result:?}");
        let endpoint_result = fixture.finish();
        assert!(endpoint_result.is_ok(), "local endpoint failed");
        let Ok(endpoint) = endpoint_result else {
            return;
        };
        assert_eq!(endpoint.queued_count(), 0);
        assert_eq!(output, b"available\n");
    }

    #[test]
    fn local_endpoint_remains_ready_across_control_connections() {
        let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let root = env::temp_dir().join(format!(
            "omarchy-quickshare-running-{}-{sequence}",
            process::id()
        ));
        let create_result = fs::create_dir_all(&root);
        assert!(create_result.is_ok(), "failed to create endpoint fixture");
        let socket = root.join("control.sock");
        let listener_result = UnixListener::bind(&socket);
        assert!(listener_result.is_ok(), "failed to bind endpoint fixture");
        let Ok(listener) = listener_result else {
            return;
        };
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            let mut endpoint = Daemon::new();
            endpoint
                .serve_until(&listener, || worker_stop.load(Ordering::Relaxed))
        });

        for _request in 0_usize..2_usize {
            let mut output = Vec::new();
            let result = omarchy_quickshare::run(
                &[String::from("--runtime-status")],
                Path::new("."),
                &socket,
                &mut output,
            );
            assert!(result.is_ok(), "runtime query failed: {result:?}");
            assert_eq!(output, b"available\n");
        }

        stopped.store(true, Ordering::Relaxed);
        let endpoint_result = worker
            .join()
            .map_err(|_panic| io::Error::other("local endpoint panicked"));
        assert!(endpoint_result.is_ok(), "local endpoint failed to stop");
        let Ok(run_result) = endpoint_result else {
            return;
        };
        assert!(run_result.is_ok(), "local endpoint returned an error");
        let cleanup_result = fs::remove_dir_all(root);
        assert!(cleanup_result.is_ok(), "failed to clean endpoint fixture");
    }
}
