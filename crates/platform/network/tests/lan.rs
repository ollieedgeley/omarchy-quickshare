//! TCP adapter behavior through real loopback sockets.

#![expect(
    clippy::expect_used,
    clippy::tests_outside_test_module,
    reason = "Integration-test entry points are tests by definition"
)]

use core::net::{Ipv4Addr, SocketAddrV4};
use std::io::{Read as _, Write as _};

use if_addrs as _;
use mdns_sd as _;
use quickshare_network::lan::{Listener, connect};

#[test]
fn listener_accepts_a_connected_stream_without_blocking_when_empty() {
    let listener = Listener::bind_any().expect("listener should bind");
    assert!(
        listener
            .accept()
            .expect("empty accept should work")
            .is_none(),
        "an empty nonblocking listener must not invent a connection"
    );
    let route = SocketAddrV4::new(Ipv4Addr::LOCALHOST, listener.port());
    let mut client = connect(route).expect("client should connect");
    let mut server = listener
        .accept()
        .expect("accept should work")
        .expect("connected stream should be pending");

    client.write_all(b"ping").expect("client should write");
    let mut received = [0_u8; 4];
    server
        .read_exact(&mut received)
        .expect("server should read");
    assert_eq!(received, *b"ping");
}
