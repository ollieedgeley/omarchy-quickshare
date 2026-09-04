#![expect(
    clippy::expect_used,
    reason = "Contract tests require descriptive adapter failures"
)]
#![expect(
    clippy::assertions_on_result_states,
    reason = "Timeout cases assert the failure result"
)]

use core::net::Ipv4Addr;
use core::time::Duration;

use if_addrs as _;
use mdns_sd as _;
use zbus as _;

use quickshare_network::network_manager::{
    Credentials, Medium, NetworkManager, Role,
};

use super::fake::FakeNetworkManager;

fn credentials() -> Credentials {
    Credentials::new(String::from("QuickShare"), String::from("passphrase"))
        .with_gateway(Ipv4Addr::new(10, 42, 0, 1))
        .with_port(1234)
}

#[test]
fn hotspot_client_session_exposes_candidate_and_cleans_up() {
    let fake = FakeNetworkManager::start(true);
    let manager =
        NetworkManager::at(fake.address()).expect("connect to fake bus");
    let session = manager
        .join_hotspot(&credentials(), Duration::from_secs(1))
        .expect("hotspot client should activate");
    let candidate = session.candidate().clone();
    assert_eq!(candidate.medium(), Medium::Hotspot);
    assert_eq!(candidate.role(), Role::Client);
    assert_eq!(candidate.addresses(), &[Ipv4Addr::new(10, 42, 0, 2)]);
    assert_eq!(candidate.gateway(), Some(Ipv4Addr::new(10, 42, 0, 1)));
    assert_eq!(candidate.port(), Some(1234));
    session.disconnect().expect("disconnect should clean up");
    assert_eq!(fake.leftover_profiles(), 0);
}

#[test]
fn hotspot_owner_session_uses_owner_role() {
    let fake = FakeNetworkManager::start(true);
    let manager =
        NetworkManager::at(fake.address()).expect("connect to fake bus");
    let session = manager
        .start_hotspot(&credentials(), Duration::from_secs(1))
        .expect("hotspot owner should activate");
    assert_eq!(session.candidate().medium(), Medium::Hotspot);
    assert_eq!(session.candidate().role(), Role::Owner);
    drop(session);
    assert_eq!(fake.leftover_profiles(), 0);
}

#[test]
fn hotspot_join_times_out_without_leaving_a_profile() {
    let fake = FakeNetworkManager::start(false);
    let manager =
        NetworkManager::at(fake.address()).expect("connect to fake bus");
    assert!(
        manager
            .join_hotspot(&credentials(), Duration::ZERO)
            .is_err()
    );
    assert_eq!(fake.leftover_profiles(), 0);
}

#[test]
fn wifi_direct_client_and_owner_sessions_clean_up() {
    let fake = FakeNetworkManager::start(true);
    let manager =
        NetworkManager::at(fake.address()).expect("connect to fake bus");
    let discovery = manager
        .find_wifi_direct_peers(Duration::from_secs(1))
        .expect("p2p find should start");
    let peer = discovery
        .next_peer(Duration::from_secs(1))
        .expect("peer lookup should work")
        .expect("peer should be visible");
    assert_eq!(peer.address(), "AA:BB:CC:DD:EE:FF");
    discovery.stop().expect("find should stop");
    let client = manager
        .join_wifi_direct(&peer, &credentials(), Duration::from_secs(1))
        .expect("p2p client should activate");
    assert_eq!(client.candidate().medium(), Medium::WifiDirect);
    assert_eq!(client.candidate().role(), Role::Client);
    client.disconnect().expect("p2p client cleanup");
    let owner = manager
        .start_wifi_direct(&credentials(), Duration::from_secs(1))
        .expect("p2p owner should activate");
    assert_eq!(owner.candidate().role(), Role::Owner);
    drop(owner);
    assert_eq!(fake.leftover_profiles(), 0);
}

#[test]
fn wifi_direct_join_times_out_without_leaving_a_profile() {
    let fake = FakeNetworkManager::start(false);
    let manager =
        NetworkManager::at(fake.address()).expect("connect to fake bus");
    let discovery = manager
        .find_wifi_direct_peers(Duration::ZERO)
        .expect("p2p find should start");
    let peer = discovery
        .next_peer(Duration::from_secs(1))
        .expect("peer lookup should work")
        .expect("started find should list the peer");
    assert!(
        manager
            .join_wifi_direct(&peer, &credentials(), Duration::ZERO)
            .is_err()
    );
    assert_eq!(fake.leftover_profiles(), 0);
}
