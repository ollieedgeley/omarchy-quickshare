use prost::Message as _;
use prost_types as _;
use quickshare_wire::{framing, sharing::Frame};

const SHARING_FIXTURES: &[&[u8]] = &[
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/apk.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/file.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/text.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/introductions/url.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/responses/accept.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/responses/cancel.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/responses/not-enough-space.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/responses/reject.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/responses/timed-out.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "incoming/responses/unsupported.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "outgoing/introductions/file.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "outgoing/introductions/text.bin"
    )),
    include_bytes!(concat!(
        "../../../../../tests/fixtures/sharing/google-v1/",
        "outgoing/introductions/url.bin"
    )),
];

#[test]
fn decodes_committed_google_sharing_fixtures() {
    for fixture in SHARING_FIXTURES {
        let decoded = Frame::decode(*fixture);

        assert!(decoded.is_ok(), "fixture must decode as a Sharing Frame");
        let Ok(frame) = decoded else {
            return;
        };
        assert!(
            frame.v1.is_some(),
            "fixture must contain a V1 Sharing frame"
        );
    }
}

#[test]
fn frames_a_message_with_a_bounded_big_endian_length_prefix() {
    let payload = [0xCA, 0xFE];
    let encoded = framing::encode(&payload, 2);

    assert_eq!(encoded, Ok(vec![0, 0, 0, 2, 0xCA, 0xFE]));
    assert_eq!(
        framing::decode(&[0, 0, 0, 2, 0xCA, 0xFE], 2),
        Ok(&payload[..])
    );
    assert_eq!(
        framing::decode(&[0, 0, 0, 3, 0xCA, 0xFE], 2),
        Err(framing::Error::LimitExceeded)
    );
}
