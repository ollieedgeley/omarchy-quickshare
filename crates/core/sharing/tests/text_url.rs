//! Public text and URL Sharing protocol contracts.

#![expect(
    clippy::absolute_paths,
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::tests_outside_test_module,
    reason = "Integration tests name std I/O types at the crate boundary"
)]

mod common;

use base64 as _;
use common::{decode_google_offer, google_v1, paired_loopback};
use prost as _;
use quickshare_sharing::{
    IncomingOffer, OfferKind, ProtocolError, SharingSession,
};
use quickshare_wire::sharing::connection_response_frame as response;
use rand_core as _;
use serde as _;

fn assert_offer(
    offer: &IncomingOffer,
    kind: OfferKind,
    name: &str,
    size: i64,
    payload_id: i64,
) {
    assert_eq!(offer.kind(), kind);
    assert_eq!(offer.name(), name);
    assert_eq!(offer.size_bytes(), size);
    assert_eq!(offer.payload_id(), payload_id);
}

fn accept_named_offer(
    session: &mut SharingSession,
    kind: OfferKind,
    name: &str,
) -> IncomingOffer {
    let offer = session.receive_incoming_offer().expect("receive offer");
    assert_eq!(offer.kind(), kind);
    assert_eq!(offer.name(), name);
    session.accept_incoming_offer().expect("accept offer");
    offer
}

#[test]
fn decodes_google_text_and_url_introductions() {
    assert_offer(
        &decode_google_offer(
            google_v1!("incoming/introductions/text.bin"),
            "Google incoming text introduction",
        ),
        OfferKind::Text,
        "fixture text",
        12,
        202,
    );

    assert_offer(
        &decode_google_offer(
            google_v1!("incoming/introductions/url.bin"),
            "Google incoming URL introduction",
        ),
        OfferKind::Url,
        "https://x.y",
        11,
        203,
    );

    assert_offer(
        &decode_google_offer(
            google_v1!("outgoing/introductions/text.bin"),
            "Google outgoing text introduction",
        ),
        OfferKind::Text,
        "fixture text",
        12,
        400,
    );

    assert_offer(
        &decode_google_offer(
            google_v1!("outgoing/introductions/url.bin"),
            "Google outgoing URL introduction",
        ),
        OfferKind::Url,
        "https://x.y",
        11,
        400,
    );
}

#[test]
fn accepts_google_apk_introduction_as_a_file() {
    let offer = decode_google_offer(
        google_v1!("incoming/introductions/apk.bin"),
        "Google APK introduction",
    );
    assert_eq!(offer.kind(), OfferKind::AndroidApp);
    assert!(offer.kind().persists_as_file());
    assert_eq!(offer.name(), "FixtureApp.apk");
    assert_eq!(offer.size_bytes(), 16);
    assert_eq!(offer.payload_id(), 204);
    assert_eq!(offer.package_name(), Some("dev.fixture.app"));
}

#[test]
fn loopback_sends_plain_text_after_consent() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let offer = accept_named_offer(
            &mut session,
            OfferKind::Text,
            "hello from omarchy",
        );
        assert_eq!(
            session
                .receive_incoming_text(&offer, |_| {}, || false)
                .expect("receive text"),
            "hello from omarchy"
        );
    });
    session
        .send_outgoing_text("hello from omarchy", || {}, |_| {}, || false)
        .expect("send text after accept");
    receiver.join().expect("receiver completes");
}

#[test]
fn loopback_sends_url_after_consent() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let offer = accept_named_offer(
            &mut session,
            OfferKind::Url,
            "https://omarchy.local",
        );
        assert_eq!(
            session
                .receive_incoming_url(&offer, |_| {}, || false)
                .expect("receive url"),
            "https://omarchy.local"
        );
    });
    session
        .send_outgoing_url("https://omarchy.local", || {}, |_| {}, || false)
        .expect("send url after accept");
    receiver.join().expect("receiver completes");
}

#[test]
fn text_rejection_reaches_the_outbound_sender() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.reject_incoming_offer().expect("reject offer");
    });
    let error = session
        .send_outgoing_text("rejected text", || {}, |_| {}, || false)
        .expect_err("peer rejection");
    assert!(matches!(error, ProtocolError::Rejected));
    receiver.join().expect("receiver completes");
}

#[test]
fn keepalive_before_text_introduction_is_ignored() {
    let (mut session, receiver) = paired_loopback(|mut session| {
        let offer = session.receive_incoming_offer().expect("receive offer");
        assert_eq!(offer.kind(), OfferKind::Text);
        session.accept_incoming_offer().expect("accept offer");
        assert_eq!(
            session
                .receive_incoming_text(&offer, |_| {}, || false)
                .expect("receive text"),
            "ping"
        );
    });
    session.send_keepalive(1).expect("send keepalive");
    session
        .send_outgoing_text("ping", || {}, |_| {}, || false)
        .expect("send text after keepalive");
    receiver.join().expect("receiver completes");
}

#[test]
fn timed_out_and_unsupported_responses_are_distinct() {
    assert_eq!(
        SharingSession::decode_response(google_v1!(
            "incoming/responses/timed-out.bin"
        ))
        .expect("Google timed-out response"),
        quickshare_wire::sharing::connection_response_frame::Status::TimedOut
    );
    assert_eq!(
        SharingSession::decode_response(google_v1!(
            "incoming/responses/unsupported.bin"
        ))
        .expect("Google unsupported response"),
        response::Status::UnsupportedAttachmentType
    );

    let (mut session, receiver) = paired_loopback(|mut session| {
        let _offer = session.receive_incoming_offer().expect("receive offer");
        session.timeout_incoming_offer().expect("timeout offer");
    });
    let error = session
        .send_outgoing_text("late", || {}, |_| {}, || false)
        .expect_err("peer timeout");
    assert!(matches!(error, ProtocolError::TimedOut));
    receiver.join().expect("receiver completes");
}
