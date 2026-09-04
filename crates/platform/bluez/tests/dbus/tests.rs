#![expect(
    clippy::expect_used,
    reason = "Contract tests require descriptive adapter failures"
)]

use core::time::Duration;

use async_io as _;
use futures_lite as _;
use quickshare_connections as _;

use quickshare_bluez::{Address, ErrorKind};

use super::fake::{
    BLE_ADDRESS, BLE_SERVICE_DATA, CLASSIC_ADDRESS, FakeBluez, OTHER_ADDRESS,
};

fn parse_address(value: &str) -> Address {
    let mut bytes = [0_u8; 6];
    let mut parts = value.split(':');
    for byte in &mut bytes {
        let part = parts.next().expect("address octet");
        *byte = u8::from_str_radix(part, 16).expect("hex octet");
    }
    Address::from_bytes(bytes)
}

#[test]
fn one_client_discovers_ble_and_classic_without_start_conflict() {
    let fake = FakeBluez::start();
    let adapter = quickshare_bluez::Adapter::on_bus(fake.address())
        .expect("connect to fake BlueZ");
    let mut scan = adapter
        .scan_ble(Duration::from_millis(400))
        .expect("BLE scan should start");
    let mut discovery = adapter
        .discover_classic(Duration::from_millis(400))
        .expect("Classic discovery should share the same StartDiscovery");
    assert_eq!(fake.start_count(), 1);
    let ble = scan
        .next_candidate()
        .expect("BLE scan should remain healthy")
        .expect("Quick Share BLE device should be found");
    let classic = discovery
        .next_candidate()
        .expect("Classic discovery should remain healthy")
        .expect("Quick Share Classic device should be found");
    assert_eq!(ble.address(), parse_address(BLE_ADDRESS));
    assert_eq!(ble.service_data(), BLE_SERVICE_DATA);
    assert_eq!(classic.address(), parse_address(CLASSIC_ADDRESS));
    assert_ne!(classic.address(), parse_address(OTHER_ADDRESS));
    discovery.stop().expect("Classic lease should release");
    scan.stop().expect("BLE lease should release");
    assert_eq!(fake.stop_count(), 1);
}
#[test]
fn empty_ble_poll_keeps_classic_discovery_responsive() {
    let fake = FakeBluez::start();
    let adapter = quickshare_bluez::Adapter::on_bus(fake.address())
        .expect("connect to fake BlueZ");
    let mut scan = adapter
        .scan_ble(Duration::from_secs(2))
        .expect("BLE scan should start");
    let mut discovery = adapter
        .discover_classic(Duration::from_secs(2))
        .expect("Classic discovery should share the scan");
    let _ble = scan
        .next_candidate()
        .expect("BLE scan should remain healthy")
        .expect("first BLE candidate");

    assert_eq!(
        scan.next_candidate().expect("empty BLE poll"),
        None,
        "empty BLE polling must be nonblocking"
    );
    let classic = discovery
        .next_candidate()
        .expect("Classic discovery should remain healthy")
        .expect("Classic candidate should remain reachable");

    assert_eq!(classic.address(), parse_address(CLASSIC_ADDRESS));
}

#[test]
fn malformed_ble_candidate_is_reported_only_once() {
    let fake = FakeBluez::start();
    let adapter = quickshare_bluez::Adapter::on_bus(fake.address())
        .expect("connect to fake BlueZ");
    let mut scan = adapter
        .scan_ble(Duration::from_secs(2))
        .expect("BLE scan should start");
    let _valid = scan
        .next_candidate()
        .expect("initial BLE scan should remain healthy")
        .expect("initial BLE candidate");
    fake.add_malformed_device();

    let error = scan
        .next_candidate()
        .expect_err("malformed service data should be reported");
    assert_eq!(error.kind(), ErrorKind::Protocol);
    assert_eq!(
        scan.next_candidate()
            .expect("malformed candidate is retired"),
        None
    );
}

#[test]
fn dropping_both_leases_stops_discovery_once() {
    let fake = FakeBluez::start();
    let adapter = quickshare_bluez::Adapter::on_bus(fake.address())
        .expect("connect to fake BlueZ");
    let scan = adapter
        .scan_ble(Duration::from_millis(200))
        .expect("BLE scan should start");
    let discovery = adapter
        .discover_classic(Duration::from_millis(200))
        .expect("Classic discovery should start");
    assert_eq!(fake.start_count(), 1);
    drop(scan);
    drop(discovery);
    assert_eq!(fake.stop_count(), 1);
    let _scan = adapter
        .scan_ble(Duration::from_millis(200))
        .expect("a new scan should StartDiscovery again");
    assert_eq!(fake.start_count(), 2);
}

#[test]
fn classic_discovery_excludes_non_quick_share_devices() {
    let fake = FakeBluez::start();
    let adapter = quickshare_bluez::Adapter::on_bus(fake.address())
        .expect("connect to fake BlueZ");
    let mut discovery = adapter
        .discover_classic(Duration::ZERO)
        .expect("Classic discovery should start");
    let classic = discovery
        .next_candidate()
        .expect("Classic discovery should remain healthy")
        .expect("Quick Share Classic device should be found");
    assert_eq!(classic.address(), parse_address(CLASSIC_ADDRESS));
    let error = discovery
        .next_candidate()
        .expect_err("unrelated Classic devices must not be listed");
    assert_eq!(error.kind(), ErrorKind::Timeout);
}
