use alloc::sync::Arc;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::time::Duration;
use std::fs;
use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

use quickshare_control::codec::{read_response, write_request};
use quickshare_control::request::Envelope as Command;
use quickshare_control::response::{Envelope, Response};
use quickshare_sharing::{EndpointSnapshot, VisibilityState};

use crate::config::Config;
use crate::daemon::Daemon;
use crate::daemon::network::{NetworkEvent, NetworkWorker};
use crate::daemon::visibility::clock::Clock;
use crate::daemon::visibility::{Resources, Visibility};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Fixture {
    daemon: Daemon,
    clock: Clock,
    released: Arc<AtomicUsize>,
    listener: UnixListener,
    root: PathBuf,
}

impl Fixture {
    pub(crate) fn new(config: Config) -> Self {
        let released = Arc::new(AtomicUsize::new(0));
        let leases = Arc::clone(&released);
        Self::with_activation(config, released, move |_| {
            Ok(Resources::fake(Arc::clone(&leases), None))
        })
    }

    pub(crate) fn with_activation(
        mut config: Config,
        released: Arc<AtomicUsize>,
        activate: impl FnMut(u64) -> Result<Resources, String> + Send + 'static,
    ) -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join(format!("qs-visibility-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&root).expect("isolated fixture directory");
        config.receive_directory = root.join("received");
        fs::create_dir_all(&config.receive_directory)
            .expect("receive directory");
        let listener = UnixListener::bind(root.join("control.sock"))
            .expect("control listener");
        let clock = Clock::fake(1000);
        let visibility = Visibility::with_clock(clock.clone());
        let worker =
            NetworkWorker::start_test(visibility, config.clone(), activate)
                .expect("fake platform worker");
        let mut daemon = Daemon::with_network_worker(worker);
        daemon.simulated = true;
        daemon.install_config(config);
        let mut fixture = Self {
            daemon,
            clock,
            released,
            listener,
            root,
        };
        let _seen =
            fixture.command(Command::simulate_peer_seen("pixel-8", "Peer"));
        fixture
    }

    pub(crate) fn command(&mut self, request: Command) -> Envelope {
        let socket = self.root.join("control.sock");
        std::thread::scope(|scope| {
            let client = scope.spawn(move || {
                let mut stream = UnixStream::connect(socket)
                    .expect("local control connection");
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .expect("bounded control response");
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .expect("bounded control request");
                write_request(&mut stream, &request)
                    .expect("write control command");
                read_response(&mut BufReader::new(stream))
                    .expect("read control response")
            });
            self.daemon
                .serve_next(&self.listener)
                .expect("serve actual daemon control");
            client.join().expect("control client")
        })
    }

    pub(crate) fn snapshot(&mut self) -> EndpointSnapshot {
        let response = self.command(Command::snapshot());
        let snapshot = match response.response() {
            Response::Snapshot { snapshot, .. } => Some(snapshot.clone()),
            _ => None,
        };
        snapshot.expect("expected endpoint snapshot")
    }

    pub(crate) fn wait_for(
        &mut self,
        predicate: impl Fn(&EndpointSnapshot) -> bool,
    ) -> EndpointSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = self.snapshot();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(
                Instant::now() < deadline,
                "daemon did not reach expected state: {snapshot:?}"
            );
            std::thread::yield_now();
        }
    }

    pub(crate) fn open(&mut self) -> EndpointSnapshot {
        assert!(matches!(
            self.command(Command::open_visibility()).response(),
            Response::Applied
        ));
        self.wait_for(|snapshot| snapshot.visibility() == VisibilityState::Open)
    }

    pub(crate) fn offer(&mut self) -> u64 {
        assert!(matches!(
            self.command(Command::simulate_incoming_text("hello"))
                .response(),
            Response::Applied
        ));
        self.snapshot()
            .active_share()
            .expect("admitted share")
            .id()
            .get()
    }

    pub(crate) fn configure(
        &mut self,
        discoverable: bool,
        consent_timeout_secs: u64,
    ) {
        self.daemon.install_config(Config {
            discoverable,
            consent_timeout_secs,
            receive_directory: self.root.join("received"),
            ..Config::default()
        });
    }

    pub(crate) fn events(&self) -> mpsc::Sender<NetworkEvent> {
        self.daemon
            .network
            .as_ref()
            .expect("network worker")
            .test_events()
    }

    pub(crate) fn visibility(&self) -> Visibility {
        self.daemon.visibility.clone()
    }
    pub(crate) const fn clock(&self) -> &Clock {
        &self.clock
    }
    pub(crate) fn released(&self) -> usize {
        self.released.load(Ordering::Acquire)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Fixture cleanup needs only Drop's destructor hook"
)]
impl Drop for Fixture {
    fn drop(&mut self) {
        self.daemon.visibility.close();
        drop(self.daemon.network.take());
        fs::remove_dir_all(&self.root).expect("fixture cleanup");
    }
}
