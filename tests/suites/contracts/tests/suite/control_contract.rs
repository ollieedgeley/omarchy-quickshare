#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::pattern_type_mismatch,
    clippy::pub_with_shorthand,
    reason = "Assert borrowed replies/fixtures; keep helpers parent-visible"
)]
mod tests {
    extern crate alloc;

    use alloc::sync::Arc;
    use core::slice;
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use core::time::Duration;
    use std::env;
    use std::fs;
    use std::io::{self, BufReader};
    use std::io::{BufRead as _, Write as _};
    use std::net::Shutdown;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::process;
    use std::thread::{self, JoinHandle};
    use std::time::Instant;

    use omarchy_quickshare::daemon::Daemon;
    use quickshare_control::{
        PROTOCOL_VERSION, client, codec, request, response,
    };
    use quickshare_sharing::DiscoveryState;

    const REQUEST_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v7/submit-text-request.jsonl"
    );
    const RESPONSE_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v7/submit-text-queued-response.jsonl"
    );
    const URL_REQUEST_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v7/submit-url-request.jsonl"
    );
    const STATUS_REQUEST_FIXTURE: &str =
        include_str!("../../../../fixtures/control/v7/status-request.jsonl");
    const STATUS_RESPONSE_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v7/status-ready-response.jsonl"
    );
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

    struct EndpointListenerFixture {
        listener: UnixListener,
        root: PathBuf,
        socket: PathBuf,
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

    impl EndpointListenerFixture {
        fn bind() -> io::Result<Self> {
            let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let root = env::temp_dir().join(format!(
                "omarchy-quickshare-running-{}-{sequence}",
                process::id()
            ));
            fs::create_dir_all(&root)?;
            let socket = root.join("control.sock");
            let listener = UnixListener::bind(&socket)?;
            Ok(Self {
                listener,
                root,
                socket,
            })
        }

        fn disconnect_client(&self, request: &str) -> io::Result<()> {
            let mut client = UnixStream::connect(&self.socket)?;
            client.write_all(request.as_bytes())?;
            client.shutdown(Shutdown::Both)
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
            let EndpointListenerFixture {
                listener,
                root,
                socket,
            } = EndpointListenerFixture::bind()?;
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

    #[test]
    fn shared_client_snapshot_observes_changed_discovery() -> io::Result<()> {
        let EndpointListenerFixture {
            listener,
            root,
            socket,
        } = EndpointListenerFixture::bind()?;
        let worker = thread::spawn(move || {
            let mut endpoint = Daemon::new();
            endpoint.serve_next(&listener)?;
            endpoint.serve_next(&listener)?;
            io::Result::Ok(())
        });

        let changed =
            client::exchange(&socket, &request::Envelope::discover())?;
        let observed =
            client::exchange(&socket, &request::Envelope::snapshot())?;
        worker
            .join()
            .map_err(|_panic| io::Error::other("local endpoint panicked"))??;
        fs::remove_dir_all(root)?;

        assert!(matches!(changed.response(), response::Response::Applied));
        let response::Response::Snapshot { snapshot, .. } = observed.response()
        else {
            panic!("expected full endpoint snapshot, got {observed:?}");
        };
        assert_eq!(snapshot.discovery(), DiscoveryState::Searching);
        Ok(())
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "One lifecycle proves rejection preserves work and IDs"
    )]
    fn active_submission_rejects_replacement_without_consuming_an_id()
    -> io::Result<()> {
        with_running_endpoint(|socket| {
            let first = client::exchange(
                socket,
                &request::Envelope::submit_text("first", "accepted-first"),
            )?;
            let response::Response::Queued { operation } = first.response()
            else {
                panic!("expected queued operation: {first:?}");
            };
            assert_eq!(operation.id_string, "1");
            assert_eq!(operation.request_id.as_deref(), Some("accepted-first"));
            let rejected = client::exchange(
                socket,
                &request::Envelope::submit_text("replacement", "rejected-busy"),
            )?;
            assert!(matches!(
                rejected.response(),
                response::Response::Error {
                    code: response::ErrorCode::Busy
                }
            ));
            let observed =
                client::exchange(socket, &request::Envelope::snapshot())?;
            let response::Response::Snapshot {
                snapshot,
                operations,
                ..
            } = observed.response()
            else {
                panic!("expected snapshot: {observed:?}");
            };
            let active =
                snapshot.active_share().expect("original remains active");
            assert_eq!(active.id_string(), "1");
            assert_eq!(
                active.attachment(),
                &quickshare_sharing::Attachment::text("first")
            );
            assert_eq!(operations.as_slice(), slice::from_ref(operation));
            let cancelled =
                client::exchange(socket, &request::Envelope::cancel(1))?;
            assert!(matches!(
                cancelled.response(),
                response::Response::Cancelled
            ));
            let second = client::exchange(
                socket,
                &request::Envelope::submit_text("second", "accepted-second"),
            )?;
            let response::Response::Queued {
                operation: second_operation,
            } = second.response()
            else {
                panic!("expected second queued operation: {second:?}");
            };
            assert_eq!(second_operation.id_string, "2");
            assert_eq!(
                second_operation.request_id.as_deref(),
                Some("accepted-second")
            );
            Ok(())
        })
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
        let arguments = vec![String::from("send"), String::from(value)];

        let run_result = omarchy_quickshare_cli::run(
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
        let actual: serde_json::Value =
            serde_json::from_str(&request).expect("valid submitted envelope");
        let mut expected: serde_json::Value =
            serde_json::from_str(expected_request)
                .expect("valid request fixture");
        let request_id =
            actual["request_id"].as_str().expect("submit request ID");
        assert!(!request_id.is_empty());
        assert!(request_id.len() <= 128);
        expected["request_id"] = actual["request_id"].clone();
        assert_eq!(actual, expected);
        assert_eq!(
            output,
            format!("Share 1 queued.\nrequest_id={request_id}\n").as_bytes()
        );
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
             \"version\":{PROTOCOL_VERSION}}}\n"
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
             \"version\":{PROTOCOL_VERSION}}}\n"
        );
        assert_submission_from("photo.jpg", &fixture.root, &expected);
        let cleanup_result = fixture.cleanup();
        assert!(cleanup_result.is_ok(), "failed to clean file fixture");
    }

    #[test]
    fn runtime_status_reports_an_available_local_endpoint() {
        let fake_result = ControlDaemonFake::start();
        assert!(fake_result.is_ok(), "failed to start control fake");
        let Ok(fake) = fake_result else {
            return;
        };
        let arguments = vec![String::from("health")];
        let mut output = Vec::new();

        let result = omarchy_quickshare_cli::run(
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
    fn real_local_endpoint_accepts_the_queued_share() {
        let fixture_result = LocalEndpointFixture::start();
        assert!(fixture_result.is_ok(), "failed to start local endpoint");
        let Ok(fixture) = fixture_result else {
            return;
        };
        let mut output = Vec::new();
        let arguments =
            vec![String::from("send"), String::from("hello from Omarchy")];

        let run_result = omarchy_quickshare_cli::run(
            &arguments,
            Path::new("."),
            fixture.socket(),
            &mut output,
        );

        assert!(run_result.is_ok(), "submission failed: {run_result:?}");
        let endpoint_result = fixture.finish();
        assert!(endpoint_result.is_ok(), "local endpoint failed");
        let output_text = String::from_utf8(output).expect("UTF-8 CLI output");
        let request_id = output_text
            .strip_prefix("Share 1 queued.\nrequest_id=")
            .and_then(|value| value.strip_suffix('\n'))
            .expect("queued operation and request ID");
        assert!(!request_id.is_empty());
        assert!(request_id.len() <= 128);
    }

    #[test]
    fn real_local_endpoint_reports_ready() {
        let fixture_result = LocalEndpointFixture::start();
        assert!(fixture_result.is_ok(), "failed to start local endpoint");
        let Ok(fixture) = fixture_result else {
            return;
        };
        let mut output = Vec::new();
        let arguments = vec![String::from("health")];

        let run_result = omarchy_quickshare_cli::run(
            &arguments,
            Path::new("."),
            fixture.socket(),
            &mut output,
        );

        assert!(run_result.is_ok(), "runtime query failed: {run_result:?}");
        let endpoint_result = fixture.finish();
        assert!(endpoint_result.is_ok(), "local endpoint failed");
        assert_eq!(output, b"available\n");
    }

    #[test]
    fn local_endpoint_remains_ready_across_control_connections()
    -> io::Result<()> {
        let fixture = EndpointListenerFixture::bind()?;
        assert_endpoint_remains_ready(fixture);
        Ok(())
    }

    #[test]
    fn local_endpoint_survives_client_closed_before_response() -> io::Result<()>
    {
        let fixture = EndpointListenerFixture::bind()?;
        fixture.disconnect_client(STATUS_REQUEST_FIXTURE)?;
        assert_endpoint_remains_ready(fixture);
        Ok(())
    }

    #[test]
    fn local_endpoint_survives_empty_disconnected_client() -> io::Result<()> {
        let fixture = EndpointListenerFixture::bind()?;
        fixture.disconnect_client("")?;
        assert_endpoint_remains_ready(fixture);
        Ok(())
    }

    #[test]
    fn unsupported_version_is_rejected_without_stopping_daemon()
    -> io::Result<()> {
        let EndpointListenerFixture {
            listener,
            root,
            socket,
        } = EndpointListenerFixture::bind()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            Daemon::new()
                .serve_until(&listener, || worker_stop.load(Ordering::Relaxed))
        });
        let observed = (|| -> io::Result<(String, String)> {
            let mut invalid = UnixStream::connect(&socket)?;
            invalid.set_read_timeout(Some(Duration::from_secs(2)))?;
            invalid.write_all(
                b"{\"request\":{\"type\":\"status\"},\"version\":255}\n",
            )?;
            let mut rejection = String::new();
            let _rejection_read =
                BufReader::new(invalid).read_line(&mut rejection)?;
            let mut valid = UnixStream::connect(&socket)?;
            valid.set_read_timeout(Some(Duration::from_secs(2)))?;
            valid.write_all(STATUS_REQUEST_FIXTURE.as_bytes())?;
            let mut ready = String::new();
            let _ready_read = BufReader::new(valid).read_line(&mut ready)?;
            Ok((rejection, ready))
        })();
        stopped.store(true, Ordering::Relaxed);
        let outcome = worker
            .join()
            .map_err(|_panic| io::Error::other("local endpoint panicked"))?;
        fs::remove_dir_all(root)?;
        let (rejection, ready) = observed?;
        assert!(
            rejection.contains("\"code\":\"unsupported_version\""),
            "expected structured unsupported_version rejection, \
             got {rejection:?}"
        );
        assert_eq!(ready, STATUS_RESPONSE_FIXTURE);
        outcome
    }

    #[test]
    fn partial_client_does_not_block_independent_status() -> io::Result<()> {
        let EndpointListenerFixture {
            listener,
            root,
            socket,
        } = EndpointListenerFixture::bind()?;
        let mut partial = UnixStream::connect(&socket)?;
        partial.write_all(b"{\"request\":")?;
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            Daemon::new()
                .serve_until(&listener, || worker_stop.load(Ordering::Relaxed))
        });
        let observed = (|| -> io::Result<String> {
            let mut valid = UnixStream::connect(&socket)?;
            valid.set_read_timeout(Some(Duration::from_secs(1)))?;
            valid.write_all(STATUS_REQUEST_FIXTURE.as_bytes())?;
            let mut ready = String::new();
            let _read = BufReader::new(valid).read_line(&mut ready)?;
            Ok(ready)
        })();
        stopped.store(true, Ordering::Relaxed);
        drop(partial);
        let outcome = worker
            .join()
            .map_err(|_panic| io::Error::other("local endpoint panicked"))?;
        fs::remove_dir_all(root)?;
        assert_eq!(observed?, STATUS_RESPONSE_FIXTURE);
        outcome
    }

    #[test]
    fn shared_client_deadline_is_not_extended_by_reply_fragments()
    -> io::Result<()> {
        let (stream, mut peer) = UnixStream::pair()?;
        let worker = thread::spawn(move || -> io::Result<()> {
            let mut request = String::new();
            let _read =
                BufReader::new(peer.try_clone()?).read_line(&mut request)?;
            for _fragment in 0_u8..7 {
                if peer.write_all(b" ").is_err() {
                    break;
                }
                thread::sleep(Duration::from_secs(1));
            }
            Ok(())
        });
        let start = Instant::now();
        let result =
            client::exchange_stream(stream, &request::Envelope::status());
        let elapsed = start.elapsed();
        worker
            .join()
            .map_err(|_panic| io::Error::other("peer panicked"))??;
        let error = result.expect_err("trickled reply must time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            elapsed < Duration::from_secs(6),
            "deadline extended to {elapsed:?}"
        );
        Ok(())
    }

    #[test]
    fn codec_preserves_fragmented_and_coalesced_frames() -> io::Result<()> {
        let (stream, mut peer) = UnixStream::pair()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let worker = thread::spawn(move || -> io::Result<()> {
            peer.write_all(b"{\"request\":{\"type\":\"sta")?;
            thread::sleep(Duration::from_millis(10));
            peer.write_all(
                format!(
                    concat!(
                        "tus\"}},\"version\":{version}}}\n",
                        "{{\"request\":{{\"type\":\"snapshot\"}},",
                        "\"version\":{version}}}\n",
                    ),
                    version = PROTOCOL_VERSION,
                )
                .as_bytes(),
            )
        });
        let mut reader = BufReader::new(stream);
        let first = codec::read_request(&mut reader)?;
        let second = codec::read_request(&mut reader)?;
        worker
            .join()
            .map_err(|_panic| io::Error::other("peer panicked"))??;
        assert!(matches!(first.request(), request::Request::Status));
        assert!(matches!(second.request(), request::Request::Snapshot));
        Ok(())
    }

    #[test]
    fn daemon_accepts_exact_frame_limit_and_rejects_one_byte_more()
    -> io::Result<()> {
        with_running_endpoint(|socket| {
            let mut frame =
                STATUS_REQUEST_FIXTURE.trim_end().as_bytes().to_vec();
            frame.resize(quickshare_control::MAX_MESSAGE_BYTES, b' ');
            frame.push(b'\n');
            assert_eq!(raw_reply(socket, &frame)?, STATUS_RESPONSE_FIXTURE);
            frame.insert(frame.len() - 1, b' ');
            let rejected = raw_reply(socket, &frame)?;
            assert_error_code(&rejected, "message_too_large")?;
            assert_eq!(
                raw_reply(socket, STATUS_REQUEST_FIXTURE.as_bytes())?,
                STATUS_RESPONSE_FIXTURE
            );
            Ok(())
        })
    }

    #[test]
    fn malformed_and_truncated_frames_leave_daemon_ready() -> io::Result<()> {
        with_running_endpoint(|socket| {
            assert_error_code(
                &raw_reply(socket, b"{broken\n")?,
                "invalid_request",
            )?;
            assert_error_code(
                &raw_reply(
                    socket,
                    format!(
                        concat!(
                            "{{\"version\":{},",
                            "\"request\":{{\"type\":\"unknown\"}}}}\n",
                        ),
                        PROTOCOL_VERSION,
                    )
                    .as_bytes(),
                )?,
                "invalid_request",
            )?;
            assert_error_code(
                &raw_reply(
                    socket,
                    format!(
                        concat!(
                            "{{\"version\":{},",
                            "\"request\":{{\"type\":\"status\"}},",
                            "\"extra\":true}}\n",
                        ),
                        PROTOCOL_VERSION,
                    )
                    .as_bytes(),
                )?,
                "invalid_request",
            )?;
            let mut truncated = UnixStream::connect(socket)?;
            truncated.set_read_timeout(Some(Duration::from_secs(2)))?;
            truncated
                .write_all(STATUS_REQUEST_FIXTURE.trim_end().as_bytes())?;
            truncated.shutdown(Shutdown::Write)?;
            let mut reply = String::new();
            let _read = BufReader::new(truncated).read_line(&mut reply)?;
            assert_error_code(&reply, "invalid_request")?;
            assert_eq!(
                raw_reply(socket, STATUS_REQUEST_FIXTURE.as_bytes())?,
                STATUS_RESPONSE_FIXTURE
            );
            Ok(())
        })
    }

    pub(super) fn raw_reply(socket: &Path, frame: &[u8]) -> io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(7)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        stream.write_all(frame)?;
        let mut response = String::new();
        let _read = BufReader::new(stream).read_line(&mut response)?;
        Ok(response)
    }

    pub(super) fn assert_error_code(frame: &str, code: &str) -> io::Result<()> {
        let response: serde_json::Value = serde_json::from_str(frame)?;
        assert_eq!(response["version"], PROTOCOL_VERSION);
        assert_eq!(response["response"]["type"], "error");
        assert_eq!(response["response"]["code"], code);
        Ok(())
    }

    pub(super) fn with_running_endpoint(
        observe: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<()> {
        let EndpointListenerFixture {
            listener,
            root,
            socket,
        } = EndpointListenerFixture::bind()?;
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            Daemon::new()
                .serve_until(&listener, || worker_stop.load(Ordering::Relaxed))
        });
        let observation = observe(&socket);
        stopped.store(true, Ordering::Relaxed);
        let outcome = worker
            .join()
            .map_err(|_panic| io::Error::other("local endpoint panicked"))?;
        fs::remove_dir_all(root)?;
        observation?;
        outcome
    }

    fn assert_endpoint_remains_ready(fixture: EndpointListenerFixture) {
        let EndpointListenerFixture {
            listener,
            root,
            socket,
        } = fixture;
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            let mut endpoint = Daemon::new();
            endpoint
                .serve_until(&listener, || worker_stop.load(Ordering::Relaxed))
        });

        for _request in 0_usize..2_usize {
            let mut output = Vec::new();
            let result = omarchy_quickshare_cli::run(
                &[String::from("health")],
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

#[cfg(test)]
#[path = "control_contract/framing.rs"]
mod framing;
