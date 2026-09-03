use super::data;
use quickshare_wire::connections::payload_transfer_frame::{
    PayloadHeader, payload_chunk,
};

#[expect(
    clippy::expect_used,
    reason = "The frame factory contract requires each nested message"
)]
fn flags(last: bool) -> i32 {
    data(PayloadHeader::default(), 0, &[], last)
        .v1
        .expect("v1 frame")
        .payload_transfer
        .expect("payload transfer")
        .payload_chunk
        .expect("payload chunk")
        .flags
        .expect("explicit flags")
}

#[test]
fn data_sets_explicit_chunk_flags() {
    assert_eq!(flags(false), 0_i32);
    assert_eq!(flags(true), payload_chunk::Flags::LastChunk as i32);
}
