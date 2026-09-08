use super::*;
use tracing_subscriber::prelude::*;

struct CancelAtReceiveBoundary(TransferCancellation);

#[derive(Eq, PartialEq)]
enum Stage {
    Transfer,
    Staging,
}

#[derive(Eq, PartialEq)]
enum Operation {
    Receive,
    Commit,
}

#[derive(Eq, PartialEq)]
enum Outcome {
    Completed,
    Published,
}

#[derive(Default)]
struct Boundary {
    stage: Option<Stage>,
    operation: Option<Operation>,
    outcome: Option<Outcome>,
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Visit forwards non-string fields to record_debug by default"
)]
impl tracing::field::Visit for Boundary {
    fn record_debug(
        &mut self,
        _: &tracing::field::Field,
        _: &dyn core::fmt::Debug,
    ) {
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "stage" => {
                self.stage = match value {
                    "transfer" => Some(Stage::Transfer),
                    "staging" => Some(Stage::Staging),
                    _ => None,
                };
            }
            "operation" => {
                self.operation = match value {
                    "receive" => Some(Operation::Receive),
                    "commit" => Some(Operation::Commit),
                    _ => None,
                };
            }
            "outcome" => {
                self.outcome = match value {
                    "completed" => Some(Outcome::Completed),
                    "success" => Some(Outcome::Published),
                    _ => None,
                };
            }
            _ => {}
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Layer defaults preserve filtering and subscriber composition"
)]
impl<S> tracing_subscriber::Layer<S> for CancelAtReceiveBoundary
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut boundary = Boundary::default();
        event.record(&mut boundary);
        if boundary.stage == Some(Stage::Transfer)
            && boundary.operation == Some(Operation::Receive)
            && boundary.outcome == Some(Outcome::Completed)
        {
            assert!(
                self.0.cancel(41),
                "cancellation before commit was too late"
            );
        }
    }
}

struct CancelDuringCommit {
    cancellation: TransferCancellation,
    result: mpsc::Sender<thread::JoinHandle<bool>>,
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Layer defaults preserve filtering and subscriber composition"
)]
impl<S> tracing_subscriber::Layer<S> for CancelDuringCommit
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut boundary = Boundary::default();
        event.record(&mut boundary);
        if boundary.stage == Some(Stage::Staging)
            && boundary.operation == Some(Operation::Commit)
            && boundary.outcome == Some(Outcome::Published)
        {
            let cancellation = self.cancellation.clone();
            let (started, attempted) = mpsc::sync_channel(0);
            let worker = thread::spawn(move || {
                started.send(()).expect("commit still running");
                cancellation.cancel(41)
            });
            attempted.recv().expect("cancel attempt at commit boundary");
            self.result.send(worker).expect("cancel worker");
        }
    }
}

#[test]
fn cancellation_after_receiving_bytes_prevents_file_commit() {
    file_terminal_ordering(true);
}

#[test]
fn file_commit_and_peer_completion_make_cancellation_too_late() {
    file_terminal_ordering(false);
}

fn file_terminal_ordering(cancel_before_commit: bool) {
    let _serial = NETWORK_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = match listener.local_addr().expect("address") {
        std::net::SocketAddr::V4(address) => Some(address),
        std::net::SocketAddr::V6(_) => None,
    }
    .expect("IPv4 listener");
    let directory = receive_directory();
    let inbound_directory = directory.clone();
    let source_directory = receive_directory();
    let path = source_directory.join("boundary.txt");
    fs::write(&path, b"complete contents").expect("source");
    let cancellation = TransferCancellation::default();
    let receiver_cancellation = cancellation.clone();
    let (race_sender, race_receiver) = mpsc::channel();
    let dispatch = tracing::Dispatch::new(
        tracing_subscriber::registry()
            .with(
                cancel_before_commit
                    .then(|| CancelAtReceiveBoundary(cancellation.clone())),
            )
            .with((!cancel_before_commit).then(|| CancelDuringCommit {
                cancellation: cancellation.clone(),
                result: race_sender,
            })),
    );
    let (_commands, command_receiver) = mpsc::channel();
    let (events, event_receiver) = mpsc::channel();
    let visibility = open_test_visibility();
    let receiver_visibility = visibility.clone();
    let inbound = thread::spawn(move || {
        tracing::dispatcher::with_default(&dispatch, || {
            let (stream, _) = listener.accept().expect("accept");
            receive_share(
                stream,
                Medium::WifiLan,
                &command_receiver,
                &events,
                &receiver_cancellation,
                &Config {
                    receive_directory: inbound_directory,
                    ..Config::default()
                },
                None,
                &receiver_visibility,
                receiver_visibility.generation_if_open().expect("visible"),
                &mut |_| true,
            )
        })
    });
    let consent = thread::spawn(move || {
        accept_inbound_offer(event_receiver, &visibility);
    });
    let mut outbound = OutboundState::default();
    outbound.remember_file(
        7,
        quickshare_storage::OutboundSource::open(&path).expect("source"),
    );
    outbound.remember_peer("loopback", PeerRoute::Lan(address));
    let transfer = outbound.transfer(7, "loopback").expect("transfer");
    let (outbound_events, _receiver) = mpsc::channel();
    let outbound_cancellation = TransferCancellation::default();
    let outbound_terminal = outbound_event(
        7,
        &transfer,
        &outbound_events,
        &outbound_cancellation,
        None,
        None,
        "Sender",
    );
    let terminal = inbound.join().expect("receiver");
    consent.join().expect("consent");
    assert!(!cancellation.cancel(41), "receive outcome was not retained");
    assert!(
        !outbound_cancellation.cancel(7),
        "send outcome was not retained"
    );
    if cancel_before_commit {
        assert!(
            matches!(terminal, NetworkEvent::InboundCancelled { share_id: 41 }),
            "{terminal:?}"
        );
        assert!(
            !directory.join("boundary.txt").exists(),
            "cancelled file was published"
        );
        assert_eq!(
            fs::read_dir(&directory).expect("receive directory").count(),
            0,
            "partial file survived cancellation"
        );
    } else {
        assert!(
            !race_receiver
                .recv_timeout(core::time::Duration::from_secs(5))
                .expect("contested cancellation worker")
                .join()
                .expect("contested cancellation result"),
            "cancellation overtook committed contents"
        );
        assert!(
            matches!(
                terminal,
                NetworkEvent::InboundCompleted {
                    share_id: 41,
                    bytes: 17,
                    ..
                }
            ),
            "{terminal:?}"
        );
        assert!(
            matches!(
                outbound_terminal,
                NetworkEvent::OutboundCompleted {
                    share_id: 7,
                    bytes: 17
                }
            ),
            "{outbound_terminal:?}"
        );
        assert_eq!(
            fs::read(directory.join("boundary.txt")).expect("committed file"),
            b"complete contents"
        );
        assert_eq!(
            fs::read_dir(&directory).expect("receive directory").count(),
            1,
            "partial file survived commit"
        );
    }
    fs::remove_dir_all(directory).expect("cleanup receive");
    fs::remove_dir_all(source_directory).expect("cleanup source");
}
