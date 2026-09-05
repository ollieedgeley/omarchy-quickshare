#![expect(
    clippy::as_conversions,
    reason = "prost protocol enums use i32 wire values"
)]
#![expect(
    unreachable_pub,
    reason = "RawPeer crosses this private integration-test fixture boundary"
)]

use super::{
    RESPONDER_RANDOM, RESPONDER_SECRET, accept_wire, read_plain, read_raw,
    write_plain, write_raw,
};
use prost::Message as _;
use quickshare_crypto::{Handshake, SecureChannel};
use quickshare_wire::connections::{OfflineFrame, v1_frame};
use std::os::unix::net::UnixStream;

pub struct RawPeer {
    channel: SecureChannel,
    next_iv: u8,
    stream: UnixStream,
}

impl RawPeer {
    pub fn accept(mut stream: UnixStream) -> Self {
        let request = read_plain(&mut stream);
        assert_eq!(
            request.v1.and_then(|v1| v1.r#type),
            Some(v1_frame::FrameType::ConnectionRequest as i32),
            "peer must receive a Connections request before UKEY2"
        );
        let mut handshake =
            Handshake::responder(RESPONDER_RANDOM, RESPONDER_SECRET);
        handshake
            .receive(&read_raw(&mut stream))
            .expect("receive raw UKEY2 M1");
        write_raw(
            &mut stream,
            &handshake.next_message().expect("create raw UKEY2 M2"),
        );
        handshake
            .receive(&read_raw(&mut stream))
            .expect("receive raw UKEY2 M3");
        write_plain(&mut stream, &accept_wire());
        assert_eq!(
            read_plain(&mut stream),
            accept_wire(),
            "peer must receive the matching acceptance"
        );
        Self {
            channel: handshake
                .complete()
                .expect("complete UKEY2")
                .into_channel(),
            next_iv: 1,
            stream,
        }
    }

    pub fn receive_encrypted(&mut self) -> OfflineFrame {
        let bytes = self
            .channel
            .decrypt(&read_raw(&mut self.stream))
            .expect("decrypt reference-shaped frame");
        OfflineFrame::decode(bytes.as_slice()).expect("decode offline frame")
    }

    pub fn send_encrypted(&mut self, frame: &OfflineFrame) {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), [self.next_iv; 16])
            .expect("encrypt reference-shaped frame");
        self.next_iv = self.next_iv.wrapping_add(1);
        write_raw(&mut self.stream, &encrypted);
    }
}
