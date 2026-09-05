//! Public bandwidth-upgrade contracts for the Connections seam.

#![expect(
    clippy::default_numeric_fallback,
    clippy::expect_used,
    clippy::std_instead_of_core,
    clippy::tests_outside_test_module,
    clippy::too_many_lines,
    clippy::unwrap_used,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

use prost as _;
use quickshare_connections::{
    Connection, ConnectionOptions, Event, Medium, UpgradeCredentials,
    UpgradeDecision, UpgradeEvent, UpgradeState,
};
use quickshare_crypto::Handshake;
use quickshare_wire as _;
use rand_core as _;
use std::{
    net::{TcpListener, TcpStream},
    thread,
};
use tracing as _;

const INITIATOR_RANDOM: [u8; 32] = [1; 32];
const RESPONDER_RANDOM: [u8; 32] = [2; 32];
const INITIATOR_SECRET: [u8; 32] = [3; 32];
const RESPONDER_SECRET: [u8; 32] = [4; 32];

#[test]
fn upgrade_decision_prefers_higher_bandwidth_and_refuses_downgrade() {
    assert_eq!(
        UpgradeDecision::from_media(Medium::Bluetooth, Medium::WifiLan),
        UpgradeDecision::Upgrade(Medium::WifiLan)
    );
    assert_eq!(
        UpgradeDecision::from_media(Medium::WifiLan, Medium::Bluetooth),
        UpgradeDecision::Stay
    );
    assert_eq!(
        UpgradeDecision::from_media(Medium::WifiLan, Medium::WifiLan),
        UpgradeDecision::Stay
    );
}

#[test]
fn loopback_upgrade_offer_completes_then_failed_path_falls_back() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Bluetooth),
        )
        .expect("establish responder encryption");
        assert_eq!(connection.medium(), Medium::Bluetooth);
        assert_eq!(connection.upgrade_state(), UpgradeState::Idle);
        assert_eq!(
            connection.receive().expect("receive upgrade offer"),
            Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium: Medium::WifiLan,
                    credentials: UpgradeCredentials::default(),
                },
            }
        );
        assert_eq!(
            UpgradeDecision::from_media(connection.medium(), Medium::WifiLan),
            UpgradeDecision::Upgrade(Medium::WifiLan)
        );
        connection
            .complete_upgrade(Medium::WifiLan)
            .expect("accept offered medium");
        assert_eq!(connection.medium(), Medium::WifiLan);
        assert_eq!(connection.upgrade_state(), UpgradeState::Idle);
        assert_eq!(
            connection.receive().expect("receive upgrade failure"),
            Event::Upgrade {
                event: UpgradeEvent::Failure {
                    medium: Medium::WifiHotspot,
                },
            }
        );
        assert_eq!(
            connection.upgrade_state(),
            UpgradeState::Failed {
                attempted: Medium::WifiHotspot,
                fallback: Medium::WifiLan,
            }
        );
        assert_eq!(connection.medium(), Medium::WifiLan);
    });

    let stream = TcpStream::connect(address).unwrap();
    let mut connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Bluetooth),
    )
    .expect("establish initiator encryption");
    connection
        .propose_upgrade(Medium::WifiLan)
        .expect("offer LAN upgrade");
    assert_eq!(
        connection.upgrade_state(),
        UpgradeState::Offered(Medium::WifiLan)
    );
    connection
        .complete_upgrade(Medium::WifiLan)
        .expect("finish LAN upgrade");
    connection
        .fail_upgrade(Medium::WifiHotspot)
        .expect("report hotspot failure");
    assert_eq!(
        connection.upgrade_state(),
        UpgradeState::Failed {
            attempted: Medium::WifiHotspot,
            fallback: Medium::WifiLan,
        }
    );
    assert_eq!(connection.medium(), Medium::WifiLan);
    responder.join().expect("responder completes");
}

#[test]
fn loopback_upgrade_carries_hotspot_credentials() {
    use core::net::Ipv4Addr;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let credentials = UpgradeCredentials {
        frequency: Some(2412),
        gateway: Some(Ipv4Addr::new(192, 168, 43, 1)),
        ip_address: Some(Ipv4Addr::new(192, 168, 43, 1)),
        password: Some(String::from("secretpass")),
        port: Some(1234),
        ssid: Some(String::from("DIRECT-OQSR")),
        device_name: None,
        pin: None,
    };
    let offered = credentials.clone();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Ble),
        )
        .expect("establish responder encryption");
        assert_eq!(
            connection.receive().expect("receive upgrade offer"),
            Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium: Medium::WifiHotspot,
                    credentials: offered,
                },
            }
        );
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Ble),
    )
    .expect("establish initiator encryption");
    connection
        .propose_upgrade_path(Medium::WifiHotspot, &credentials)
        .expect("offer hotspot credentials");
    responder.join().expect("responder completes");
}

#[test]
fn loopback_upgrade_path_request_is_routed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Bluetooth),
        )
        .expect("establish responder encryption");
        assert_eq!(
            connection.receive().expect("receive path request"),
            Event::Upgrade {
                event: UpgradeEvent::PathRequest {
                    mediums: vec![Medium::WifiLan, Medium::WifiHotspot],
                },
            }
        );
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Bluetooth),
    )
    .expect("establish initiator encryption");
    connection
        .request_upgrade_path(&[Medium::WifiLan, Medium::WifiHotspot])
        .expect("request path");
    responder.join().expect("responder completes");
}

#[test]
fn loopback_no_upgrade_preserves_bluetooth_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection = Connection::accept(
            stream,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Bluetooth),
        )
        .expect("establish responder encryption");
        assert_eq!(
            connection.receive().expect("receive payload"),
            Event::Bytes {
                id: 7,
                bytes: b"hello".to_vec(),
            }
        );
        assert_eq!(connection.medium(), Medium::Bluetooth);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection = Connection::connect(
        stream,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Bluetooth),
    )
    .expect("establish initiator encryption");
    connection
        .send_bytes(7, b"hello")
        .expect("send without upgrade");
    responder.join().expect("responder completes");
}

#[test]
fn loopback_upgrade_io_does_not_duplicate_bytes_on_prior_channel() {
    use std::io::Read as _;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let (old_initiator, old_responder) = UnixStream::pair().unwrap();
    let (new_initiator, new_responder) = UnixStream::pair().unwrap();
    let mut prior = old_initiator.try_clone().unwrap();
    prior
        .set_read_timeout(Some(Duration::from_millis(50)))
        .unwrap();
    let responder = thread::spawn(move || {
        let mut connection = Connection::accept_io(
            old_responder,
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("responder", "responder")
                .with_medium(Medium::Bluetooth),
        )
        .expect("establish responder encryption");
        assert!(matches!(
            connection.receive().expect("path available"),
            Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium: Medium::WifiLan,
                    ..
                },
            }
        ));
        connection
            .complete_upgrade_io(Medium::WifiLan, new_responder)
            .expect("client handshake");
        assert_eq!(
            connection.receive().expect("bytes on new stream"),
            Event::Bytes {
                id: 11,
                bytes: b"after".to_vec(),
            }
        );
    });
    let mut connection = Connection::connect_io(
        old_initiator,
        Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
        ConnectionOptions::new("initiator", "initiator")
            .with_medium(Medium::Bluetooth),
    )
    .expect("establish initiator encryption");
    connection
        .propose_upgrade(Medium::WifiLan)
        .expect("offer lan");
    connection
        .complete_upgrade_io(Medium::WifiLan, new_initiator)
        .expect("host handshake");
    connection
        .send_bytes(11, b"after")
        .expect("send on new stream");
    let mut buf = [0_u8; 4];
    let read = prior.read(&mut buf).unwrap_or(0);
    assert_eq!(
        read, 0,
        "payload must not be duplicated on the prior channel"
    );
    responder.join().expect("responder completes");
}
