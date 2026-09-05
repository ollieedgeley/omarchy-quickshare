use super::{
    NetworkCommand, NetworkEvent, TransferCancellation, emit_peer_lost,
    inbound::receive_share, remember_seen, transfer::outbound_event,
};
use crate::daemon::media::PeerRoute;
use crate::daemon::outbound::OutboundState;
use core::net::{Ipv4Addr, SocketAddrV4};
use core::time::Duration;
use quickshare_connections::Medium;
use std::fs;
use std::net::TcpListener;
use std::sync::{
    Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::thread;

#[test]
fn stopping_discovery_emits_peer_lost_for_remembered_peers() {
    let (sender, receiver) = mpsc::channel();
    let mut seen = std::collections::HashSet::new();
    remember_seen(
        &mut seen,
        &NetworkEvent::PeerSeen {
            name: String::from("phone"),
            peer_id: String::from("peer-a"),
            route: PeerRoute::Lan(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 9)),
        },
    );
    assert!(emit_peer_lost(&mut seen, &sender));
    assert!(seen.is_empty());
    let event = receiver.recv().expect("peer lost");
    assert!(matches!(
        event,
        NetworkEvent::PeerLost { peer_id } if peer_id == "peer-a"
    ));
    assert!(receiver.try_recv().is_err());
}

static NEXT_RECEIVE_DIRECTORY: AtomicU64 = AtomicU64::new(0);
static NETWORK_TEST_LOCK: Mutex<()> = Mutex::new(());

fn receive_directory() -> std::path::PathBuf {
    let sequence = NEXT_RECEIVE_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "omarchy-quickshare-worker-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("receive directory");
    path
}

fn receive_once(
    listener: TcpListener,
    command_receiver: mpsc::Receiver<NetworkCommand>,
    inbound_sender: mpsc::Sender<NetworkEvent>,
    receive_directory: std::path::PathBuf,
) -> NetworkEvent {
    let (stream, _) = listener.accept().expect("inbound accept");
    receive_share(
        stream,
        Medium::WifiLan,
        &command_receiver,
        &inbound_sender,
        &TransferCancellation::default(),
        &receive_directory,
        Duration::from_secs(2),
        None,
        &mut |_| true,
    )
}

fn accept_until_completed(
    inbound_receiver: mpsc::Receiver<NetworkEvent>,
    command_sender: mpsc::Sender<NetworkCommand>,
) {
    loop {
        match inbound_receiver.recv() {
            Ok(NetworkEvent::InboundOffered { .. }) => {
                command_sender
                    .send(NetworkCommand::AcceptInbound { share_id: 1 })
                    .expect("accept inbound");
            }
            Ok(NetworkEvent::InboundCompleted { kind, bytes, .. }) => {
                assert!(bytes > 0);
                assert!(
                    kind == quickshare_sharing::OfferKind::Text
                        || kind == quickshare_sharing::OfferKind::Url
                        || kind == quickshare_sharing::OfferKind::File
                        || kind == quickshare_sharing::OfferKind::AndroidApp
                );
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

fn assert_single_terminal(
    terminal: &NetworkEvent,
    inbound: &NetworkEvent,
    receiver: &mpsc::Receiver<NetworkEvent>,
) {
    assert!(
        matches!(
            terminal,
            NetworkEvent::OutboundCompleted { share_id: 7, .. }
        ),
        "{terminal:?}; inbound={inbound:?}"
    );
    let mut terminals = 1_u8;
    while let Ok(event) = receiver.try_recv() {
        if matches!(
            event,
            NetworkEvent::OutboundCompleted { .. }
                | NetworkEvent::OutboundFailed { .. }
                | NetworkEvent::OutboundCancelled { .. }
                | NetworkEvent::OutboundRejected { .. }
        ) {
            terminals = terminals.saturating_add(1);
        }
    }
    assert_eq!(terminals, 1);
}

fn send_payload_family(payload: fn(&mut OutboundState, u64)) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let ipv4 = match address {
        std::net::SocketAddr::V4(addr) => *addr.ip(),
        std::net::SocketAddr::V6(_) => Ipv4Addr::LOCALHOST,
    };
    let receive_directory = receive_directory();
    let (command_sender, command_receiver) = mpsc::channel();
    let (inbound_sender, inbound_receiver) = mpsc::channel();
    let inbound = thread::spawn(move || {
        receive_once(
            listener,
            command_receiver,
            inbound_sender,
            receive_directory,
        )
    });
    let consent = thread::spawn(move || {
        accept_until_completed(inbound_receiver, command_sender);
    });
    let mut outbound = OutboundState::default();
    payload(&mut outbound, 7);
    outbound.remember_peer(
        "loopback",
        PeerRoute::Lan(SocketAddrV4::new(ipv4, address.port())),
    );
    let transfer = outbound.transfer(7, "loopback").expect("transfer");
    let (event_sender, event_receiver) = mpsc::channel();
    let terminal = outbound_event(
        7,
        &transfer,
        &event_sender,
        &TransferCancellation::default(),
        None,
        None,
    );
    let inbound_event = inbound.join().expect("inbound thread");
    consent.join().expect("consent thread");
    assert_single_terminal(&terminal, &inbound_event, &event_receiver);
}

#[test]
fn text_url_and_file_payloads_complete_once_over_the_worker() {
    let _serial = NETWORK_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    send_payload_family(|outbound, share_id| {
        outbound.remember_text(share_id, String::from("hello"));
    });
    send_payload_family(|outbound, share_id| {
        outbound.remember_url(share_id, String::from("https://example.test"));
    });
    send_payload_family(|outbound, share_id| {
        let path = receive_directory().join("note.txt");
        fs::write(&path, b"abcd").expect("file");
        let source =
            quickshare_storage::OutboundSource::open(&path).expect("source");
        outbound.remember_file(share_id, source);
    });
}

fn reject_until_finished(
    inbound_receiver: mpsc::Receiver<NetworkEvent>,
    command_sender: mpsc::Sender<NetworkCommand>,
) {
    loop {
        match inbound_receiver.recv() {
            Ok(NetworkEvent::InboundOffered { .. }) => {
                command_sender
                    .send(NetworkCommand::RejectInbound { share_id: 1 })
                    .expect("reject inbound");
            }
            Ok(NetworkEvent::InboundRejected { .. }) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

#[test]
fn rejected_offer_does_not_retry_another_route() {
    let _serial = NETWORK_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let ipv4 = match address {
        std::net::SocketAddr::V4(addr) => *addr.ip(),
        std::net::SocketAddr::V6(_) => Ipv4Addr::LOCALHOST,
    };
    let receive_directory = receive_directory();
    let (command_sender, command_receiver) = mpsc::channel();
    let (inbound_sender, inbound_receiver) = mpsc::channel();
    let inbound = thread::spawn(move || {
        receive_once(
            listener,
            command_receiver,
            inbound_sender,
            receive_directory,
        )
    });
    let consenter = thread::spawn(move || {
        reject_until_finished(inbound_receiver, command_sender);
    });
    let mut outbound = OutboundState::default();
    outbound.remember_text(7, String::from("hello"));
    outbound.remember_peer(
        "loopback",
        PeerRoute::Lan(SocketAddrV4::new(ipv4, address.port())),
    );
    outbound.remember_peer(
        "loopback",
        PeerRoute::Lan(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1)),
    );
    let transfer = outbound.transfer(7, "loopback").expect("transfer");
    let (event_sender, _event_receiver) = mpsc::channel();
    let terminal = outbound_event(
        7,
        &transfer,
        &event_sender,
        &TransferCancellation::default(),
        None,
        None,
    );
    let inbound_event = inbound.join().expect("inbound thread");
    consenter.join().expect("consent thread");
    assert!(
        matches!(terminal, NetworkEvent::OutboundRejected { share_id: 7 }),
        "{terminal:?}; inbound={inbound_event:?}"
    );
}

fn diagnostic_subscriber(
    file: fs::File,
) -> impl tracing::Subscriber + Send + Sync {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(
            "omarchy_quickshare=debug",
        ))
        .with_ansi(false)
        .without_time()
        .with_target(true)
        .with_writer(Mutex::new(file))
        .finish()
}

fn receive_with_diagnostics(
    dispatch: tracing::Dispatch,
    listener: TcpListener,
    command_receiver: mpsc::Receiver<NetworkCommand>,
    inbound_sender: mpsc::Sender<NetworkEvent>,
    receive_directory: std::path::PathBuf,
) -> NetworkEvent {
    tracing::dispatcher::with_default(&dispatch, || {
        receive_once(
            listener,
            command_receiver,
            inbound_sender,
            receive_directory,
        )
    })
}

fn accept_inbound_offer(
    inbound_receiver: mpsc::Receiver<NetworkEvent>,
    command_sender: mpsc::Sender<NetworkCommand>,
) {
    while let Ok(event) = inbound_receiver.recv() {
        if matches!(event, NetworkEvent::InboundOffered { .. }) {
            command_sender
                .send(NetworkCommand::AcceptInbound { share_id: 41 })
                .expect("accept inbound");
            break;
        }
    }
}

fn send_private_text(
    address: SocketAddrV4,
    dispatch: &tracing::Dispatch,
) -> NetworkEvent {
    let mut outbound = OutboundState::default();
    outbound.remember_text(7, String::from("privacy-sentinel-text"));
    outbound.remember_peer("private-peer-sentinel", PeerRoute::Lan(address));
    let transfer = outbound
        .transfer(7, "private-peer-sentinel")
        .expect("transfer");
    let (event_sender, _event_receiver) = mpsc::channel();
    tracing::dispatcher::with_default(dispatch, || {
        outbound_event(
            7,
            &transfer,
            &event_sender,
            &TransferCancellation::default(),
            None,
            None,
        )
    })
}

fn assert_diagnostic_observations(
    diagnostic_path: &std::path::Path,
    private_directory: &str,
) {
    let observations =
        fs::read_to_string(diagnostic_path).expect("diagnostic observations");
    let started = observations
        .lines()
        .find(|line| {
            line.contains("direction=\"inbound\"")
                && line.contains("initial_medium=\"wifi_lan\"")
                && line.contains("connection_id=")
                && line.contains("stage=\"connection\"")
                && line.contains("operation=\"receive\"")
                && line.contains("outcome=\"started\"")
        })
        .expect("inbound connection start");
    let completed = observations
        .lines()
        .find(|line| {
            line.contains("share_id=41")
                && line.contains("stage=\"connection\"")
                && line.contains("operation=\"receive\"")
                && line.contains("outcome=\"completed\"")
        })
        .expect("inbound connection completion");
    let started_position = observations
        .find(started)
        .expect("connection start position");
    let assigned_position = observations
        .find("share_id=41")
        .expect("share assignment position");
    let completed_position = observations
        .find(completed)
        .expect("connection completion position");
    assert!(
        started_position < assigned_position
            && assigned_position <= completed_position,
        "connection chronology was out of order: {observations}",
    );
    assert!(
        !started.contains("share_id=41"),
        "start unexpectedly had an assigned share: {started}",
    );
    assert!(
        !observations.contains("privacy-sentinel-text"),
        "diagnostics leaked text payload: {observations}",
    );
    assert!(
        !observations.contains("private-peer-sentinel"),
        "diagnostics leaked peer name: {observations}",
    );
    assert!(
        !observations.contains(private_directory),
        "diagnostics leaked receive directory: {observations}",
    );
}

#[test]
fn inbound_connection_span_correlates_handshake_assignment_and_terminal() {
    let _serial = NETWORK_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
    let address = listener.local_addr().expect("listen address");
    let receive_directory = receive_directory();
    let private_directory = receive_directory.to_string_lossy().into_owned();
    let (command_sender, command_receiver) = mpsc::channel();
    let (inbound_sender, inbound_receiver) = mpsc::channel();
    let diagnostic_path = receive_directory.join("diagnostics.log");
    let diagnostic_file =
        fs::File::create(&diagnostic_path).expect("diagnostic file");
    let dispatch =
        tracing::Dispatch::new(diagnostic_subscriber(diagnostic_file));
    tracing::callsite::rebuild_interest_cache();
    let inbound_dispatch = dispatch.clone();
    let inbound = thread::spawn(move || {
        receive_with_diagnostics(
            inbound_dispatch,
            listener,
            command_receiver,
            inbound_sender,
            receive_directory,
        )
    });
    let consent = thread::spawn(move || {
        accept_inbound_offer(inbound_receiver, command_sender);
    });
    assert!(address.is_ipv4(), "listener was not IPv4: {address}");
    let std::net::SocketAddr::V4(address) = address else {
        return;
    };
    let terminal = send_private_text(address, &dispatch);
    let inbound_terminal = inbound.join().expect("inbound thread");
    consent.join().expect("consent thread");
    drop(dispatch);
    assert!(
        matches!(
            terminal,
            NetworkEvent::OutboundCompleted { share_id: 7, .. }
        ),
        "unexpected outbound terminal: {terminal:?}",
    );
    assert!(
        matches!(
            inbound_terminal,
            NetworkEvent::InboundCompleted { share_id: 41, .. }
        ),
        "unexpected inbound terminal: {inbound_terminal:?}",
    );
    assert_diagnostic_observations(&diagnostic_path, &private_directory);
}
