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
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

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

fn receive_directory() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "omarchy-quickshare-worker-{}-{}",
        std::process::id(),
        Instant::now().elapsed().as_nanos()
    ));
    fs::create_dir_all(&path).expect("receive directory");
    path
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
    });
    let consent = thread::spawn(move || {
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
                            || kind
                                == quickshare_sharing::OfferKind::AndroidApp
                    );
                    break;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
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
    assert!(
        matches!(
            terminal,
            NetworkEvent::OutboundCompleted { share_id: 7, .. }
        ),
        "{terminal:?}; inbound={inbound_event:?}"
    );
    let mut terminals = 1_u8;
    while let Ok(event) = event_receiver.try_recv() {
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

#[test]
fn text_url_and_file_payloads_complete_once_over_the_worker() {
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

#[test]
fn rejected_offer_does_not_retry_another_route() {
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
    });
    let consenter = thread::spawn(move || {
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
