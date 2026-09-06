use super::super::inbound::receive_share;
use super::super::{NetworkCommand, NetworkEvent, TransferCancellation};
use crate::config::Config;
use crate::daemon::media::{
    PeerRoute, connect_route, initiate_bandwidth_upgrade, sharing_session,
};
use crate::daemon::visibility::Visibility;
use crate::daemon::visibility::tests::Fixture;
use core::time::Duration;
use quickshare_connections::Medium;
use quickshare_control::request::Envelope as Command;
use quickshare_control::response::Response;
use quickshare_sharing::ProtocolError;
use quickshare_sharing::{Phase, VisibilityState};
use std::net::{SocketAddr, SocketAddrV4, TcpListener};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

const PAYLOAD: &str = "accepted bytes survive visibility revocation";

fn receive(
    events: Sender<NetworkEvent>,
    visibility: Visibility,
    config: Config,
) -> (
    SocketAddrV4,
    Sender<NetworkCommand>,
    JoinHandle<NetworkEvent>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let address = match listener.local_addr().expect("address") {
        SocketAddr::V4(address) => Some(address),
        SocketAddr::V6(_) => None,
    }
    .expect("IPv4 listener");
    let generation =
        visibility.generation_if_open().expect("active generation");
    let (commands, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let (stream, _) = listener.accept().expect("incoming connection");
        receive_share(
            stream,
            Medium::WifiLan,
            &receiver,
            &events,
            &TransferCancellation::default(),
            &config,
            None,
            &visibility,
            generation,
            &mut |_| true,
        )
    });
    (address, commands, worker)
}

fn send(
    address: SocketAddrV4,
    accepted: Sender<()>,
    resume: Receiver<()>,
) -> JoinHandle<Result<(), ProtocolError>> {
    thread::spawn(move || {
        let mut connection =
            connect_route(None, &PeerRoute::Lan(address), "Sender")?;
        let _upgrade =
            initiate_bandwidth_upgrade(&mut connection, None, "Sender")?;
        let mut session = sharing_session(connection);
        let _pairing = session
            .exchange_account_free_pairing()
            .map_err(quickshare_sharing::PairingError::into_source)?;
        session.send_outgoing_text(
            PAYLOAD,
            || {
                accepted.send(()).expect("accepted notification");
                resume.recv().expect("resume payload");
            },
            |_| {},
            || false,
        )
    })
}

#[derive(Clone, Copy)]
enum Revocation {
    Expiry,
    Stop,
    Off,
}

fn revoke(fixture: &mut Fixture, revocation: Revocation) {
    match revocation {
        Revocation::Expiry => fixture.clock().advance(10),
        Revocation::Stop => {
            assert!(matches!(
                fixture.command(Command::close_visibility()).response(),
                Response::Applied
            ));
        }
        Revocation::Off => fixture.configure(false, 20),
    }
    assert_eq!(fixture.snapshot().visibility(), VisibilityState::Closed);
    assert_eq!(fixture.released(), 1);
}

fn config() -> Config {
    Config {
        consent_timeout_secs: 20,
        ..Config::default()
    }
}

fn accepted_payload(revocation: Revocation, expire_before_accept: bool) {
    let mut fixture = Fixture::new(config());
    if matches!(revocation, Revocation::Off) {
        fixture.configure(true, 20);
        let _opened = fixture.wait_for(|snapshot| {
            snapshot.visibility() == VisibilityState::Open
        });
    } else {
        let _opened = fixture.open();
    }
    if matches!(revocation, Revocation::Expiry) {
        fixture.clock().advance(590);
    }
    let (address, _commands, receiver) =
        receive(fixture.events(), fixture.visibility(), config());
    let (accepted, accepted_receiver) = mpsc::channel();
    let (resume, resume_receiver) = mpsc::channel();
    let sender = send(address, accepted, resume_receiver);
    let offered = fixture.wait_for(|snapshot| {
        snapshot
            .active_share()
            .is_some_and(|share| share.phase() == Phase::AwaitingLocalConsent)
    });
    let share_id = offered.active_share().expect("admitted offer").id().get();
    if expire_before_accept {
        revoke(&mut fixture, revocation);
        assert_eq!(
            fixture.snapshot().active_share().expect("pending").phase(),
            Phase::AwaitingLocalConsent
        );
    }
    assert!(matches!(
        fixture.command(Command::accept(share_id)).response(),
        Response::Applied
    ));
    accepted_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("encrypted peer acceptance");
    if !expire_before_accept {
        revoke(&mut fixture, revocation);
    }
    assert_eq!(
        fixture
            .snapshot()
            .active_share()
            .expect("accepted transfer")
            .phase(),
        Phase::Transferring
    );
    resume.send(()).expect("release payload");
    sender
        .join()
        .expect("sender thread")
        .expect("send exact text");
    let terminal = receiver.join().expect("receiver thread");
    assert!(matches!(
        &terminal,
        NetworkEvent::InboundCompleted {
            bytes, value: Some(value), share_id: id, ..
        } if *bytes == u64::try_from(PAYLOAD.len()).expect("payload length")
            && value == PAYLOAD
            && *id == share_id
    ));
    fixture
        .events()
        .send(terminal)
        .expect("deliver actual terminal");
    let _completed = fixture.wait_for(|snapshot| {
        snapshot
            .active_share()
            .is_some_and(|share| share.phase() == Phase::Completed)
    });
}

#[test]
fn accepted_encrypted_bytes_survive_passive_expiry() {
    accepted_payload(Revocation::Expiry, false);
}

#[test]
fn accepted_encrypted_bytes_survive_explicit_stop() {
    accepted_payload(Revocation::Stop, false);
}

#[test]
fn accepted_encrypted_bytes_survive_durable_off() {
    accepted_payload(Revocation::Off, false);
}

#[test]
fn admitted_pending_encrypted_offer_can_be_accepted_after_passive_expiry() {
    accepted_payload(Revocation::Expiry, true);
}

#[test]
fn explicit_stop_rejects_pending_offer_over_encrypted_connection() {
    let mut fixture = Fixture::new(config());
    let _opened = fixture.open();
    let (address, _commands, receiver) =
        receive(fixture.events(), fixture.visibility(), config());
    let (accepted, _accepted_receiver) = mpsc::channel();
    let (_resume, resume_receiver) = mpsc::channel();
    let sender = send(address, accepted, resume_receiver);
    let offered = fixture.wait_for(|snapshot| {
        snapshot
            .active_share()
            .is_some_and(|share| share.phase() == Phase::AwaitingLocalConsent)
    });
    let share_id = offered.active_share().expect("admitted offer").id().get();
    revoke(&mut fixture, Revocation::Stop);
    assert!(matches!(
        sender.join().expect("sender thread"),
        Err(ProtocolError::Rejected)
    ));
    assert!(matches!(
        receiver.join().expect("receiver thread"),
        NetworkEvent::InboundFailed {
            share_id: Some(id), reason, ..
        } if id == share_id && reason == "cancelled"
    ));
}

#[test]
fn busy_coordinator_rejects_peer_without_failing_existing_share() {
    let mut fixture = Fixture::new(config());
    let _opened = fixture.open();
    let existing = fixture.offer();
    let (address, _commands, receiver) =
        receive(fixture.events(), fixture.visibility(), config());
    let (accepted, _accepted_receiver) = mpsc::channel();
    let (_resume, resume_receiver) = mpsc::channel();
    let sender = send(address, accepted, resume_receiver);
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !sender.is_finished() {
        let _snapshot = fixture.snapshot();
        assert!(
            std::time::Instant::now() < deadline,
            "busy offer not rejected"
        );
        thread::yield_now();
    }
    assert!(matches!(
        sender.join().expect("sender thread"),
        Err(ProtocolError::Rejected)
    ));
    let terminal = receiver.join().expect("receiver thread");
    fixture
        .events()
        .send(terminal)
        .expect("deliver rejected terminal");
    let snapshot = fixture.snapshot();
    let active = snapshot.active_share().expect("original pending share");
    assert_eq!(active.id().get(), existing);
    assert_eq!(active.phase(), Phase::AwaitingLocalConsent);
}

fn occupied_worker_payload(revocation: Revocation) {
    use crate::daemon::visibility::Resources;
    use alloc::sync::Arc;
    use core::sync::atomic::AtomicUsize;

    let listener =
        TcpListener::bind("127.0.0.1:0").expect("loopback publication");
    listener.set_nonblocking(true).expect("nonblocking accept");
    let address = match listener.local_addr().expect("listener address") {
        SocketAddr::V4(address) => Some(address),
        SocketAddr::V6(_) => None,
    }
    .expect("IPv4 listener");
    let mut listener = Some(listener);
    let released = Arc::new(AtomicUsize::new(0));
    let lease_released = Arc::clone(&released);
    let mut fixture = Fixture::with_activation(config(), released, move |_| {
        Ok(Resources::fake(
            Arc::clone(&lease_released),
            listener.take(),
        ))
    });
    if matches!(revocation, Revocation::Off) {
        fixture.configure(true, 20);
        let _opened = fixture.wait_for(|snapshot| {
            snapshot.visibility() == VisibilityState::Open
        });
    } else {
        let _opened = fixture.open();
    }
    if matches!(revocation, Revocation::Expiry) {
        fixture.clock().advance(590);
    }
    let (accepted, accepted_receiver) = mpsc::channel();
    let (resume, resume_receiver) = mpsc::channel();
    let sender = send(address, accepted, resume_receiver);
    let offered = fixture.wait_for(|snapshot| {
        snapshot
            .active_share()
            .is_some_and(|share| share.phase() == Phase::AwaitingLocalConsent)
    });
    let share_id = offered.active_share().expect("worker offer").id().get();
    assert!(matches!(
        fixture.command(Command::accept(share_id)).response(),
        Response::Applied
    ));
    accepted_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("worker accepted encrypted offer");
    revoke(&mut fixture, revocation);
    assert!(
        matches!(
            std::net::TcpStream::connect_timeout(
                &SocketAddr::V4(address),
                Duration::from_secs(1)
            ),
            Err(error) if error.kind() == std::io::ErrorKind::ConnectionRefused
        ),
        "closed publication still accepted new sockets"
    );
    assert_eq!(
        fixture
            .snapshot()
            .active_share()
            .expect("occupied worker")
            .phase(),
        Phase::Transferring
    );
    resume.send(()).expect("resume encrypted payload");
    sender
        .join()
        .expect("sender thread")
        .expect("encrypted payload completed");
    let completed = fixture.wait_for(|snapshot| {
        snapshot
            .active_share()
            .is_some_and(|share| share.phase() == Phase::Completed)
    });
    let share = completed.active_share().expect("completed worker share");
    assert_eq!(share.id().get(), share_id);
    assert_eq!(
        share.transferred_bytes(),
        u64::try_from(PAYLOAD.len()).expect("payload length")
    );
    assert_eq!(
        share.attachment(),
        &quickshare_sharing::Attachment::text(PAYLOAD)
    );
}

#[test]
fn occupied_worker_releases_listener_on_stop_before_accepted_bytes_finish() {
    occupied_worker_payload(Revocation::Stop);
}

#[test]
fn occupied_worker_releases_listener_on_expiry_before_accepted_bytes_finish() {
    occupied_worker_payload(Revocation::Expiry);
}

#[test]
fn occupied_worker_releases_listener_on_off_before_accepted_bytes_finish() {
    occupied_worker_payload(Revocation::Off);
}
