use super::connection::{
    accept_connection, attempt_order, connect_connection, medium_name,
    open_visibility, sharing_session, start_discovery,
};
use super::upgrade::{
    accept_bandwidth_upgrade, accept_negotiated_upgrade,
    complete_or_fail_upgrade, initiate_bandwidth_upgrade, upgrade_decision,
};
use core::net::Ipv4Addr;
use core::time::Duration;
use quickshare_connections::{
    Event, Medium, UpgradeCredentials, UpgradeDecision, UpgradeEvent,
    UpgradeState,
};
use std::net::{IpAddr, TcpListener, TcpStream};
use std::sync::Barrier;
use std::thread;

use alloc::sync::Arc;

fn pair(session: &mut quickshare_sharing::SharingSession) {
    let _pairing = session.exchange_account_free_pairing().expect("pairing");
}

#[test]
fn upgrade_decision_prefers_higher_rank_then_fallback() {
    assert_eq!(
        upgrade_decision(Medium::Ble, Medium::WifiLan),
        UpgradeDecision::Upgrade(Medium::WifiLan)
    );
    assert_eq!(
        upgrade_decision(Medium::WifiLan, Medium::Ble),
        UpgradeDecision::Stay
    );
    assert_eq!(medium_name(Medium::WifiDirect), "wifi_direct");
    assert_eq!(attempt_order()[4], Medium::Bluetooth);
}

#[test]
fn missing_adapter_leases_close_on_drop() {
    start_discovery(None, Duration::from_secs(15)).close();
    open_visibility(None).close();
}

#[test]
fn failed_upgrade_keeps_payload_bytes_on_the_original_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        complete_or_fail_upgrade::<TcpStream>(
            &mut connection,
            Medium::WifiHotspot,
            Err(quickshare_sharing::ProtocolError::Disconnected),
        )
        .expect("record failure");
        assert_eq!(connection.medium(), Medium::Bluetooth);
        assert_eq!(
            connection.upgrade_state(),
            UpgradeState::Failed {
                attempted: Medium::WifiHotspot,
                fallback: Medium::Bluetooth,
            }
        );
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    complete_or_fail_upgrade::<TcpStream>(
        &mut connection,
        Medium::WifiHotspot,
        Err(quickshare_sharing::ProtocolError::Disconnected),
    )
    .expect("report failure");
    assert_eq!(connection.medium(), Medium::Bluetooth);
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}

#[test]
fn successful_upgrade_continues_payload_once_on_the_new_stream() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let upgrade = TcpListener::bind("127.0.0.1:0").unwrap();
    let upgrade_address = upgrade.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        assert!(matches!(
            connection.receive().expect("path available"),
            Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium: Medium::WifiLan,
                    ..
                },
            }
        ));
        let upgraded = TcpStream::connect(upgrade_address).unwrap();
        complete_or_fail_upgrade(
            &mut connection,
            Medium::WifiLan,
            Ok(upgraded),
        )
        .expect("complete");
        assert_eq!(connection.medium(), Medium::WifiLan);
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    connection
        .propose_upgrade(Medium::WifiLan)
        .expect("offer lan");
    let (upgraded, _) = upgrade.accept().unwrap();
    complete_or_fail_upgrade(&mut connection, Medium::WifiLan, Ok(upgraded))
        .expect("complete");
    assert_eq!(connection.medium(), Medium::WifiLan);
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}

#[test]
fn accept_bandwidth_upgrade_joins_lan_then_pairs() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let upgrade = TcpListener::bind("127.0.0.1:0").unwrap();
    let upgrade_address = upgrade.local_addr().unwrap();
    let ip = match upgrade_address.ip() {
        IpAddr::V4(ip) => ip,
        IpAddr::V6(_) => Ipv4Addr::LOCALHOST,
    };
    let port = upgrade_address.port();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        let wifi = accept_negotiated_upgrade(&mut connection, None)
            .expect("join offered path");
        assert!(wifi.is_none());
        assert_eq!(connection.medium(), Medium::WifiLan);
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    connection
        .propose_upgrade_path(
            Medium::WifiLan,
            &UpgradeCredentials {
                ip_address: Some(ip),
                port: Some(port),
                ..UpgradeCredentials::default()
            },
        )
        .expect("offer lan");
    let (upgraded, _) = upgrade.accept().unwrap();
    complete_or_fail_upgrade(&mut connection, Medium::WifiLan, Ok(upgraded))
        .expect("complete");
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}

#[test]
fn accept_bandwidth_upgrade_keeps_original_when_join_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let responder_barrier = Arc::clone(&barrier);
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        let _synced = responder_barrier.wait();
        let wifi = accept_bandwidth_upgrade(
            &mut connection,
            UpgradeEvent::PathAvailable {
                medium: Medium::WifiLan,
                credentials: UpgradeCredentials::default(),
            },
            None,
        )
        .expect("fallback");
        assert!(wifi.is_none());
        assert_eq!(connection.medium(), Medium::Bluetooth);
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    let _synced = barrier.wait();
    complete_or_fail_upgrade::<TcpStream>(
        &mut connection,
        Medium::WifiLan,
        Err(quickshare_sharing::ProtocolError::Disconnected),
    )
    .expect("report failure");
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}

#[test]
fn accept_negotiated_upgrade_preserves_bluetooth_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        let wifi = accept_negotiated_upgrade(&mut connection, None)
            .expect("no upgrade offer");
        assert!(wifi.is_none());
        assert_eq!(connection.medium(), Medium::Bluetooth);
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}

#[test]
fn initiate_bandwidth_upgrade_without_manager_pairs_on_original() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        let wifi = accept_negotiated_upgrade(&mut connection, None)
            .expect("fallback after failure");
        assert!(wifi.is_none());
        assert_eq!(connection.medium(), Medium::Bluetooth);
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    let wifi = initiate_bandwidth_upgrade(&mut connection, None)
        .expect("report missing manager");
    assert!(wifi.is_none());
    assert_eq!(connection.medium(), Medium::Bluetooth);
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}

#[test]
fn first_upgrade_path_failure_then_second_path_pairs() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let upgrade = TcpListener::bind("127.0.0.1:0").unwrap();
    let upgrade_address = upgrade.local_addr().unwrap();
    let ip = match upgrade_address.ip() {
        IpAddr::V4(ip) => ip,
        IpAddr::V6(_) => Ipv4Addr::LOCALHOST,
    };
    let port = upgrade_address.port();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut connection =
            accept_connection(stream, Medium::Bluetooth).expect("accept");
        let wifi = accept_negotiated_upgrade(&mut connection, None)
            .expect("join second path");
        assert!(wifi.is_none());
        assert_eq!(connection.medium(), Medium::WifiLan);
        let mut session = sharing_session(connection);
        pair(&mut session);
    });
    let stream = TcpStream::connect(address).unwrap();
    let mut connection =
        connect_connection(stream, Medium::Bluetooth).expect("connect");
    complete_or_fail_upgrade::<TcpStream>(
        &mut connection,
        Medium::WifiHotspot,
        Err(quickshare_sharing::ProtocolError::Disconnected),
    )
    .expect("fail first path");
    connection
        .propose_upgrade_path(
            Medium::WifiLan,
            &UpgradeCredentials {
                ip_address: Some(ip),
                port: Some(port),
                ..UpgradeCredentials::default()
            },
        )
        .expect("offer second path");
    let (upgraded, _) = upgrade.accept().unwrap();
    complete_or_fail_upgrade(&mut connection, Medium::WifiLan, Ok(upgraded))
        .expect("complete second path");
    assert_eq!(connection.medium(), Medium::WifiLan);
    let mut session = sharing_session(connection);
    pair(&mut session);
    responder.join().expect("responder");
}
