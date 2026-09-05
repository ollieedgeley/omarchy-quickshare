//! Classic and L2CAP adapter behavior through FakeRadio.

#![expect(
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::tests_outside_test_module,
    reason = "Integration-test entry points are tests by definition"
)]

use core::time::Duration;

use async_io as _;
use futures_lite as _;
use quickshare_connections as _;
use rustix as _;
use tracing as _;
use zbus as _;

use quickshare_bluez::testing::FakeRadio;
use quickshare_bluez::{
    Address, ErrorKind, QUICK_SHARE_BLE_UUID, ReceiverAdvertisement,
};

const LISTENER: Address =
    Address::from_bytes([0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
const INITIATOR: Address =
    Address::from_bytes([0x23, 0x34, 0x45, 0x56, 0x67, 0x78]);
const RECEIVER: Address =
    Address::from_bytes([0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
const SERVICE: &str = QUICK_SHARE_BLE_UUID;
const SERIAL_PORT: &str = "00001101-0000-1000-8000-00805f9b34fb";
const PSM: u16 = 0x1001;

#[test]
fn classic_discovery_finds_a_listener_and_exchanges_bytes() {
    let radio = FakeRadio::new();
    let listener_adapter = radio.adapter(LISTENER).expect("listener adapter");
    let initiator = radio.adapter(INITIATOR).expect("initiator adapter");
    let mut listener = listener_adapter
        .listen_classic(SERVICE)
        .expect("Classic listener should bind");
    let mut discovery = initiator
        .discover_classic(Duration::from_millis(100))
        .expect("Classic discovery should start");
    let candidate = discovery
        .next_candidate()
        .expect("discovery should remain healthy")
        .expect("listener should be found");
    assert_eq!(candidate.address(), LISTENER);
    let client = initiator
        .connect_classic(&candidate, SERVICE, Duration::from_millis(100))
        .expect("Classic connect should succeed");
    let accepted = listener
        .accept()
        .expect("listener accept should work")
        .expect("inbound Classic socket should be pending");
    client.send(b"ping").expect("initiator should write");
    let received = accepted
        .recv(Duration::from_millis(0))
        .expect("listener should read");
    assert_eq!(received, *b"ping");
}

#[test]
fn classic_discovery_times_out_without_a_listener() {
    let radio = FakeRadio::new();
    let initiator = radio.adapter(INITIATOR).expect("initiator adapter");
    let mut discovery = initiator
        .discover_classic(Duration::from_millis(25))
        .expect("Classic discovery should start");
    radio.advance(Duration::from_millis(25));
    let error = discovery
        .next_candidate()
        .expect_err("empty Classic discovery should time out");
    assert_eq!(error.kind(), ErrorKind::Timeout);
}

#[test]
fn classic_discovery_skips_non_quick_share_listeners() {
    let radio = FakeRadio::new();
    let listener_adapter = radio.adapter(LISTENER).expect("listener adapter");
    let initiator = radio.adapter(INITIATOR).expect("initiator adapter");
    let _listener = listener_adapter
        .listen_classic(SERIAL_PORT)
        .expect("serial-port listener should bind");
    let mut discovery = initiator
        .discover_classic(Duration::from_millis(25))
        .expect("Classic discovery should start");
    radio.advance(Duration::from_millis(25));
    let error = discovery
        .next_candidate()
        .expect_err("non-Quick-Share Classic devices must be excluded");
    assert_eq!(error.kind(), ErrorKind::Timeout);
}

#[test]
fn ble_and_classic_discovery_run_together() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver adapter");
    let listener_adapter = radio.adapter(LISTENER).expect("listener adapter");
    let initiator = radio.adapter(INITIATOR).expect("initiator adapter");
    let _advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(vec![0x23, 0x0A]))
        .expect("receiver should advertise");
    let _listener = listener_adapter
        .listen_classic(SERVICE)
        .expect("Classic listener should bind");
    let mut scan = initiator
        .scan_ble(Duration::from_millis(100))
        .expect("BLE scan should start beside Classic discovery");
    let mut discovery = initiator
        .discover_classic(Duration::from_millis(100))
        .expect("Classic discovery should start beside BLE scan");
    let ble = scan
        .next_candidate()
        .expect("BLE scan should remain healthy")
        .expect("advertised receiver should be found");
    let classic = discovery
        .next_candidate()
        .expect("Classic discovery should remain healthy")
        .expect("Quick Share listener should be found");
    assert_eq!(ble.address(), RECEIVER);
    assert_eq!(classic.address(), LISTENER);
}

#[test]
fn l2cap_connects_and_cleans_up_when_the_listener_stops() {
    let radio = FakeRadio::new();
    let listener_adapter = radio.adapter(LISTENER).expect("listener adapter");
    let initiator = radio.adapter(INITIATOR).expect("initiator adapter");
    let mut listener = listener_adapter
        .listen_l2cap(PSM)
        .expect("L2CAP listener should bind");
    let client = initiator
        .connect_l2cap(LISTENER, PSM, Duration::from_millis(100))
        .expect("L2CAP connect should succeed");
    let accepted = listener
        .accept()
        .expect("listener accept should work")
        .expect("inbound L2CAP channel should be pending");
    client.send(&[1, 2, 3]).expect("initiator should write");
    let received = accepted
        .recv(Duration::from_millis(0))
        .expect("listener should read");
    assert_eq!(received, [1, 2, 3]);
    listener.stop().expect("listener should unregister");
    let error = initiator
        .connect_l2cap(LISTENER, PSM, Duration::from_millis(100))
        .expect_err("stopped listener must not accept");
    assert_eq!(error.kind(), ErrorKind::Unavailable);
}
