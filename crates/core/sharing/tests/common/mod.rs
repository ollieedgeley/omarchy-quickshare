//! Loopback pairing setup shared by Sharing integration tests.

#![expect(
    clippy::arbitrary_source_item_ordering,
    clippy::impl_trait_in_params,
    clippy::inline_trait_bounds,
    clippy::needless_pass_by_value,
    clippy::pub_with_shorthand,
    clippy::single_call_fn,
    clippy::std_instead_of_core,
    unreachable_pub,
    reason = "Shared integration-test setup favors a compact scenario API"
)]

use quickshare_connections::{Connection, ConnectionOptions};
use quickshare_crypto::Handshake;
use quickshare_sharing::{IncomingOffer, SharingSession};
use std::{
    net::{SocketAddr, TcpListener, TcpStream},
    thread::{self, JoinHandle},
};

pub const INITIATOR_RANDOM: [u8; 32] = [1; 32];
pub const RESPONDER_RANDOM: [u8; 32] = [2; 32];
pub const INITIATOR_SECRET: [u8; 32] = [3; 32];
pub const RESPONDER_SECRET: [u8; 32] = [4; 32];

pub fn bind_loopback() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let address = listener.local_addr().expect("listener address");
    (listener, address)
}

pub fn accept_stream(listener: TcpListener) -> TcpStream {
    let (stream, _) = listener.accept().expect("accept peer");
    stream
}

pub fn connect_stream(address: SocketAddr) -> TcpStream {
    TcpStream::connect(address).expect("connect peer")
}

pub fn accept_session(listener: TcpListener) -> SharingSession {
    SharingSession::new(
        Connection::accept(
            accept_stream(listener),
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET),
            ConnectionOptions::new("remote", "Remote"),
        )
        .expect("establish peer session"),
    )
}

pub fn connect_session(address: SocketAddr) -> SharingSession {
    SharingSession::new(
        Connection::connect(
            connect_stream(address),
            Handshake::initiator(INITIATOR_RANDOM, INITIATOR_SECRET),
            ConnectionOptions::new("local", "Omarchy"),
        )
        .expect("establish local session"),
    )
}

pub fn pair(session: &mut SharingSession) {
    let _pairing = session.exchange_account_free_pairing().expect("pair");
}

pub fn spawn_peer<T: Send + 'static>(
    listener: TcpListener,
    body: impl FnOnce(SharingSession) -> T + Send + 'static,
) -> JoinHandle<T> {
    thread::spawn(move || body(accept_session(listener)))
}

pub fn paired_loopback<T: Send + 'static>(
    peer: impl FnOnce(SharingSession) -> T + Send + 'static,
) -> (SharingSession, JoinHandle<T>) {
    let (listener, address) = bind_loopback();
    let handle = spawn_peer(listener, |mut session| {
        pair(&mut session);
        peer(session)
    });
    let mut session = connect_session(address);
    pair(&mut session);
    (session, handle)
}

pub fn decode_google_offer(bytes: &[u8], label: &'static str) -> IncomingOffer {
    SharingSession::decode_offer(bytes).expect(label)
}

macro_rules! google_v1 {
    ($rel:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/fixtures/sharing/google-v1/",
            $rel
        ))
    };
}
pub(crate) use google_v1;
