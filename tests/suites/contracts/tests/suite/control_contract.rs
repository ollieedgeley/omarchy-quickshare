#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicUsize, Ordering};
    use std::env;
    use std::fs;
    use std::io::{self, BufReader};
    use std::io::{BufRead as _, Write as _};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::process;
    use std::thread::{self, JoinHandle};

    const REQUEST_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v1/submit-text-request.jsonl"
    );
    const RESPONSE_FIXTURE: &str = include_str!(
        "../../../../fixtures/control/v1/submit-text-queued-response.jsonl"
    );
    const EXPECTED_OUTPUT: &[u8] = b"Share queued.\n";
    static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct ControlDaemonFake {
        root: PathBuf,
        socket: PathBuf,
        worker: JoinHandle<io::Result<String>>,
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

        #[expect(
            clippy::single_call_fn,
            reason = "Named fake setup keeps the behavior test focused"
        )]
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

    #[expect(
        clippy::single_call_fn,
        reason = "The fake worker names its one external protocol interaction"
    )]
    fn serve_once(listener: &UnixListener) -> io::Result<String> {
        let (mut stream, _) = listener.accept()?;
        let mut request = String::new();
        let _bytes_read =
            BufReader::new(stream.try_clone()?).read_line(&mut request)?;
        if !request.is_empty() {
            stream.write_all(RESPONSE_FIXTURE.as_bytes())?;
        }
        Ok(request)
    }

    #[test]
    fn bare_text_is_submitted_to_the_local_endpoint() {
        let fake_result = ControlDaemonFake::start();
        assert!(fake_result.is_ok(), "failed to start control fake");
        let Ok(fake) = fake_result else {
            return;
        };
        let mut output = Vec::new();
        let arguments = vec![String::from("hello from Omarchy")];

        let run_result =
            omarchy_quickshare::run(&arguments, fake.socket(), &mut output);
        assert!(run_result.is_ok(), "text submission failed: {run_result:?}");

        let request_result = fake.finish();
        assert!(
            request_result.is_ok(),
            "control fake failed: {request_result:?}",
        );
        let Ok(request) = request_result else {
            return;
        };
        assert_eq!(request, REQUEST_FIXTURE);
        assert_eq!(output, EXPECTED_OUTPUT);
    }
}
