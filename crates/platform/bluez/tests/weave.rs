//! Weave stream and owned-fd fail-closed contracts.
#![expect(
    clippy::big_endian_bytes,
    clippy::expect_used,
    clippy::tests_outside_test_module,
    clippy::unused_trait_names,
    reason = "Integration-test entry points are tests by definition"
)]

use core::time::Duration;

use async_io as _;
use futures_lite as _;
use quickshare_connections as _;
use std::io::{Read, Write};
use zbus as _;

use quickshare_bluez::testing::FakeRadio;
use quickshare_bluez::{Address, ErrorKind, ReceiverAdvertisement};

const RECEIVER: Address =
    Address::from_bytes([0x10, 0x20, 0x30, 0x40, 0x50, 0x60]);
const SENDER: Address =
    Address::from_bytes([0x11, 0x21, 0x31, 0x41, 0x51, 0x61]);

#[test]
fn new_connection_without_fd_fails_closed() {
    let error = FakeRadio::pipe_from_new_connection(None)
        .expect_err("NewConnection must not succeed without an fd");
    assert_eq!(error.kind(), ErrorKind::Protocol);
}

#[test]
fn fake_medium_sockets_do_not_become_connection_streams() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver");
    let sender = radio.adapter(SENDER).expect("sender");
    let mut server = receiver.serve_gatt_weave().expect("gatt");
    let _advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(vec![0x23, 0x0A]))
        .expect("advertise");
    let mut scan = sender.scan_ble(Duration::from_millis(100)).expect("scan");
    let candidate = scan.next_candidate().expect("scan ok").expect("found");
    let client = sender
        .connect_gatt_weave(&candidate, Duration::from_millis(100))
        .expect("connect");
    let accepted = server.accept().expect("accept").expect("pending");
    assert_eq!(
        client.into_io().expect_err("fake weave has no fd").kind(),
        ErrorKind::Unavailable
    );
    assert_eq!(
        accepted.into_io().expect_err("fake weave has no fd").kind(),
        ErrorKind::Unavailable
    );
}

#[test]
fn dropping_a_gatt_server_unregisters_the_application() {
    let radio = FakeRadio::new();
    let receiver = radio.adapter(RECEIVER).expect("receiver");
    let sender = radio.adapter(SENDER).expect("sender");
    let server = receiver.serve_gatt_weave().expect("gatt");
    server.stop().expect("stop");
    let _advertisement = receiver
        .advertise_receiver(ReceiverAdvertisement::new(vec![0x23, 0x0A]))
        .expect("advertise");
    let mut scan = sender.scan_ble(Duration::from_millis(100)).expect("scan");
    let candidate = scan.next_candidate().expect("scan ok").expect("found");
    let error = sender
        .connect_gatt_weave(&candidate, Duration::from_millis(100))
        .expect_err("stopped GATT must not accept");
    assert_eq!(error.kind(), ErrorKind::Unavailable);
}

#[test]
fn raw_io_reads_coalesced_length_prefix_then_body() {
    let (mut writer, mut reader) =
        FakeRadio::connected_classic_io().expect("classic pair");
    let mut frame = Vec::from(4_u32.to_be_bytes());
    frame.extend_from_slice(b"abcd");
    writer.write_all(&frame).expect("write coalesced frame");
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix).expect("prefix");
    assert_eq!(u32::from_be_bytes(prefix), 4);
    let mut body = [0_u8; 4];
    reader.read_exact(&mut body).expect("body");
    assert_eq!(&body, b"abcd");
}

#[test]
fn raw_io_reads_fragmented_length_prefix_then_body() {
    let (mut writer, mut reader) =
        FakeRadio::connected_classic_io().expect("classic pair");
    writer
        .write_all(&3_u32.to_be_bytes())
        .expect("write prefix");
    let mut prefix = [0_u8; 4];
    reader.read_exact(&mut prefix).expect("prefix");
    assert_eq!(u32::from_be_bytes(prefix), 3);
    writer.write_all(b"xyz").expect("write body");
    let mut body = [0_u8; 3];
    reader.read_exact(&mut body).expect("body");
    assert_eq!(&body, b"xyz");
}
