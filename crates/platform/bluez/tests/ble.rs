//! BLE adapter behavior through the public FakeRadio seam.

#![expect(
    clippy::assertions_on_result_states,
    clippy::doc_markdown,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::similar_names,
    clippy::tests_outside_test_module,
    reason = "Integration-test entry points are tests by definition"
)]

use core::time::Duration;

use async_io as _;
use futures_lite as _;
use quickshare_connections as _;
use zbus as _;

use quickshare_bluez::testing::FakeRadio;
use quickshare_bluez::{
    Address, ErrorKind, QUICK_SHARE_BLE_UUID, ReceiverAdvertisement,
};

const RECEIVER: Address =
    Address::from_bytes([0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
const SENDER: Address =
    Address::from_bytes([0x11, 0x21, 0x31, 0x41, 0x51, 0x61]);
const PAYLOAD: &[u8] = &[0x23, 0x0A, 0x0B, 0x0C];

#[test]
fn scan_finds_an_advertised_quick_share_receiver() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver adapter");
    let sender = radio.adapter(SENDER).expect("sender adapter");
    let _advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(PAYLOAD.to_vec()))
        .expect("receiver should advertise");
    let mut scan = sender
        .scan_ble(Duration::from_millis(100))
        .expect("scan should start");
    let candidate = scan
        .next_candidate()
        .expect("scan should remain healthy")
        .expect("advertised receiver should be found");
    assert_eq!(candidate.address(), RECEIVER);
    assert_eq!(candidate.service_data(), PAYLOAD);
    assert_eq!(QUICK_SHARE_BLE_UUID, "0000fef3-0000-1000-8000-00805f9b34fb");
}

#[test]
fn scan_times_out_when_nobody_advertises() {
    let radio = FakeRadio::new();
    let sender = radio.adapter(SENDER).expect("sender adapter");
    let mut scan = sender
        .scan_ble(Duration::from_millis(40))
        .expect("scan should start");
    radio.advance(Duration::from_millis(40));
    let error = scan
        .next_candidate()
        .expect_err("empty scan should time out");
    assert_eq!(error.kind(), ErrorKind::Timeout);
}

#[test]
fn dropping_an_advertisement_cleans_up_discovery() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver adapter");
    let sender = radio.adapter(SENDER).expect("sender adapter");
    let advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(PAYLOAD.to_vec()))
        .expect("receiver should advertise");
    advertisement
        .stop()
        .expect("advertisement should unregister");
    let mut scan = sender
        .scan_ble(Duration::from_millis(40))
        .expect("scan should start");
    radio.advance(Duration::from_millis(40));
    let error = scan
        .next_candidate()
        .expect_err("cleaned advertisement must not be found");
    assert_eq!(error.kind(), ErrorKind::Timeout);
}

#[test]
fn gatt_weave_exchanges_bytes_after_discovery() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver adapter");
    let sender = radio.adapter(SENDER).expect("sender adapter");
    let mut server = receiver
        .serve_gatt_weave()
        .expect("GATT weave should listen");
    let _advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(PAYLOAD.to_vec()))
        .expect("receiver should advertise");
    let mut scan = sender
        .scan_ble(Duration::from_millis(100))
        .expect("scan should start");
    let candidate = scan
        .next_candidate()
        .expect("scan should remain healthy")
        .expect("advertised receiver should be found");
    let client = sender
        .connect_gatt_weave(&candidate, Duration::from_millis(100))
        .expect("weave connect should succeed");
    let accepted = server
        .accept()
        .expect("server accept should work")
        .expect("inbound weave socket should be pending");
    client.send(b"hello").expect("client should write");
    let received = accepted
        .recv(Duration::from_millis(0))
        .expect("server should read");
    assert_eq!(received, b"hello");
}

#[test]
fn production_system_adapter_does_not_invent_success_without_bluez() {
    if let Ok(adapter) =
        quickshare_bluez::Adapter::on_bus("unix:path=/no/such/bluez.sock")
    {
        let error = adapter
            .scan_ble(Duration::from_millis(1))
            .expect_err("missing BlueZ must not scan");
        assert_ne!(error.kind(), ErrorKind::Timeout);
    }
}

#[test]
fn weave_connect_token_rejects_malformed_input() {
    assert!(FakeRadio::parse_connect_token(&[]).is_err());
    assert!(FakeRadio::parse_connect_token(&[0x80, 0, 1]).is_err());
    let mut token = FakeRadio::encode_connect_token(RECEIVER);
    token[0] = 0x00;
    assert_eq!(
        FakeRadio::parse_connect_token(&token)
            .expect_err("bad header")
            .kind(),
        ErrorKind::Protocol
    );
    token = FakeRadio::encode_connect_token(RECEIVER);
    assert_eq!(
        FakeRadio::parse_connect_token(&token).expect("roundtrip"),
        RECEIVER
    );
}

#[test]
fn malformed_inbound_token_is_rejected_on_accept() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver adapter");
    let mut server = receiver
        .serve_gatt_weave()
        .expect("GATT weave should listen");
    radio
        .inject_connect_token(RECEIVER, vec![0xff])
        .expect("inject");
    let error = server.accept().expect_err("malformed token must fail");
    assert_eq!(error.kind(), ErrorKind::Protocol);
}

#[test]
fn gatt_weave_exchanges_a_six_byte_payload() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver adapter");
    let sender = radio.adapter(SENDER).expect("sender adapter");
    let mut server = receiver
        .serve_gatt_weave()
        .expect("GATT weave should listen");
    let _advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(PAYLOAD.to_vec()))
        .expect("receiver should advertise");
    let mut scan = sender
        .scan_ble(Duration::from_millis(100))
        .expect("scan should start");
    let candidate = scan
        .next_candidate()
        .expect("scan should remain healthy")
        .expect("advertised receiver should be found");
    let client = sender
        .connect_gatt_weave(&candidate, Duration::from_millis(100))
        .expect("weave connect should succeed");
    let accepted = server
        .accept()
        .expect("server accept should work")
        .expect("inbound weave socket should be pending");
    client
        .send(&[1, 2, 3, 4, 5, 6])
        .expect("client should write");
    let received = accepted
        .recv(Duration::from_millis(0))
        .expect("server should read");
    assert_eq!(received, [1, 2, 3, 4, 5, 6]);
}
