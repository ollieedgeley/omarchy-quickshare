use core::net::{Ipv4Addr, SocketAddrV4};

use quickshare_sharing::{Attachment, OfferKind, Phase};

use super::{
    Daemon, INBOUND_PEER_ID, INBOUND_PEER_NAME, NetworkEvent, completion_notice,
};
use crate::daemon::media::PeerRoute;

fn active_phase(daemon: &Daemon) -> Phase {
    daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("share remains visible")
        .phase()
}

#[test]
fn inbound_offer_exposes_pin_until_local_consent() {
    let mut daemon = Daemon::new();
    daemon.apply_network_event(NetworkEvent::InboundOffered {
        kind: OfferKind::File,
        name: String::from("note.txt"),
        size_bytes: 12,
        verification_code: String::from("6251"),
    });

    let share = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("inbound offer is active");
    assert_eq!(share.phase(), Phase::AwaitingLocalConsent);
    assert_eq!(share.verification_code(), Some("6251"));
    let share_id = share.id().get();

    assert!(daemon.sharing.accept_inbound(share_id));
    let accepted = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("accepted inbound share remains active");
    assert_eq!(accepted.phase(), Phase::Transferring);
    assert_eq!(accepted.verification_code(), None);
}

#[test]
fn inbound_terminal_events_preserve_rejection_and_cancellation() {
    let mut inbound = Daemon::new();
    inbound.apply_network_event(NetworkEvent::InboundOffered {
        kind: OfferKind::File,
        name: String::from("note.txt"),
        size_bytes: 12,
        verification_code: String::from("6251"),
    });
    let inbound_id = inbound
        .sharing
        .snapshot()
        .active_share()
        .expect("inbound offer")
        .id()
        .get();
    inbound.apply_network_event(NetworkEvent::InboundRejected {
        share_id: inbound_id,
    });
    assert_eq!(active_phase(&inbound), Phase::Rejected);

    let mut cancelled = Daemon::new();
    cancelled.apply_network_event(NetworkEvent::InboundOffered {
        kind: OfferKind::File,
        name: String::from("cancelled.txt"),
        size_bytes: 12,
        verification_code: String::from("9418"),
    });
    let cancelled_id = cancelled
        .sharing
        .snapshot()
        .active_share()
        .expect("inbound offer")
        .id()
        .get();
    assert!(cancelled.sharing.accept_inbound(cancelled_id));
    cancelled.apply_network_event(NetworkEvent::InboundCancelled {
        share_id: cancelled_id,
    });
    assert_eq!(active_phase(&cancelled), Phase::Cancelled);
}

#[test]
fn inbound_android_app_offer_snapshots_as_file() {
    let mut daemon = Daemon::new();
    daemon.apply_network_event(NetworkEvent::InboundOffered {
        kind: OfferKind::AndroidApp,
        name: String::from("chat.apk"),
        size_bytes: 64,
        verification_code: String::from("4820"),
    });
    let share = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("android app offer is active");
    assert_eq!(share.phase(), Phase::AwaitingLocalConsent);
    assert_eq!(share.attachment(), &Attachment::file("chat.apk", 64));
    assert_eq!(share.total_bytes(), 64);
}

#[test]
fn outbound_terminal_events_preserve_rejection_and_cancellation() {
    let mut daemon = Daemon::new();
    daemon
        .sharing
        .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
    let rejected_id = daemon
        .sharing
        .queue_outbound(Attachment::file("note.txt", 12));
    assert!(
        daemon
            .sharing
            .select_peer(rejected_id.get(), INBOUND_PEER_ID)
    );
    daemon.apply_network_event(NetworkEvent::OutboundRejected {
        share_id: rejected_id.get(),
    });
    assert_eq!(active_phase(&daemon), Phase::Rejected);

    assert!(daemon.sharing.dismiss(rejected_id.get()));
    let cancelled_id = daemon
        .sharing
        .queue_outbound(Attachment::file("cancelled.txt", 12));
    assert!(
        daemon
            .sharing
            .select_peer(cancelled_id.get(), INBOUND_PEER_ID)
    );
    daemon.apply_network_event(NetworkEvent::OutboundAccepted {
        share_id: cancelled_id.get(),
    });
    daemon.apply_network_event(NetworkEvent::OutboundCancelled {
        share_id: cancelled_id.get(),
    });
    assert_eq!(active_phase(&daemon), Phase::Cancelled);
}

#[test]
fn outbound_events_expose_pin_and_start_transfer_on_peer_acceptance() {
    let mut daemon = Daemon::new();
    daemon
        .sharing
        .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
    let share_id = daemon
        .sharing
        .queue_outbound(Attachment::file("note.txt", 12));
    assert!(daemon.sharing.select_peer(share_id.get(), INBOUND_PEER_ID));

    daemon.apply_network_event(NetworkEvent::OutboundPairing {
        share_id: share_id.get(),
        verification_code: String::from("9418"),
    });
    let pairing = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("outbound share awaits peer");
    assert_eq!(pairing.phase(), Phase::AwaitingPeerConsent);
    assert_eq!(pairing.verification_code(), Some("9418"));

    daemon.apply_network_event(NetworkEvent::OutboundAccepted {
        share_id: share_id.get(),
    });
    let transferring = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("accepted outbound share remains active");
    assert_eq!(transferring.phase(), Phase::Transferring);
    assert_eq!(transferring.verification_code(), None);

    daemon.apply_network_event(NetworkEvent::OutboundCompleted {
        bytes: 12,
        share_id: share_id.get(),
    });
    let completed = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("completed outbound share remains visible");
    assert_eq!(completed.phase(), Phase::Completed);
    assert_eq!(completed.id_string(), "1");
}

#[test]
fn file_text_and_url_each_have_one_terminal_outcome() {
    for attachment in [
        Attachment::file("note.txt", 4),
        Attachment::text("hi"),
        Attachment::url("https://example.test"),
    ] {
        let mut daemon = Daemon::new();
        daemon
            .sharing
            .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
        let share_id = daemon.sharing.queue_outbound(attachment);
        let bytes = daemon
            .sharing
            .snapshot()
            .active_share()
            .expect("queued share")
            .total_bytes();
        assert!(daemon.sharing.select_peer(share_id.get(), INBOUND_PEER_ID));
        daemon.apply_network_event(NetworkEvent::OutboundAccepted {
            share_id: share_id.get(),
        });
        daemon.apply_network_event(NetworkEvent::OutboundCompleted {
            bytes,
            share_id: share_id.get(),
        });
        assert_eq!(active_phase(&daemon), Phase::Completed);
        assert!(!daemon.sharing.fail(share_id.get()));
    }
}

#[test]
fn cancellation_is_the_only_terminal_outcome() {
    let mut daemon = Daemon::new();
    daemon
        .sharing
        .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
    let share_id = daemon
        .sharing
        .queue_outbound(Attachment::file("note.txt", 4));
    assert!(daemon.sharing.cancel(share_id.get()));
    daemon.apply_network_event(NetworkEvent::OutboundCompleted {
        bytes: 4,
        share_id: share_id.get(),
    });
    assert_eq!(active_phase(&daemon), Phase::Cancelled);
    assert!(!daemon.sharing.fail(share_id.get()));
}

#[test]
fn completion_notice_fires_only_on_successful_completion() {
    assert!(completion_notice(true, Phase::Completed));
    assert!(!completion_notice(false, Phase::Completed));
    assert!(!completion_notice(true, Phase::Cancelled));
    assert!(!completion_notice(true, Phase::Failed));
}

#[test]
fn inbound_consent_timeout_fails_the_offered_share() {
    let mut daemon = Daemon::new();
    daemon.apply_network_event(NetworkEvent::InboundOffered {
        kind: OfferKind::File,
        name: String::from("note.txt"),
        size_bytes: 12,
        verification_code: String::from("6251"),
    });
    let share_id = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("inbound offer")
        .id()
        .get();
    daemon.apply_network_event(NetworkEvent::InboundFailed {
        reason: String::from("timed_out"),
        share_id: None,
    });
    let share = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("timed out share remains visible");
    assert_eq!(share.id().get(), share_id);
    assert_eq!(share.phase(), Phase::Failed);
    assert_eq!(share.terminal_reason(), Some("timed_out"));
    assert!(share.recovery_guidance().is_some());
}

#[test]
fn inbound_accept_starts_eta_clock() {
    let mut daemon = Daemon::new();
    daemon.apply_network_event(NetworkEvent::InboundOffered {
        kind: OfferKind::File,
        name: String::from("note.txt"),
        size_bytes: 12,
        verification_code: String::from("6251"),
    });
    let share_id = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("inbound offer")
        .id()
        .get();
    assert!(
        daemon
            .share_response(&quickshare_control::request::Request::Accept {
                share_id
            })
            .expect("accept")
            .is_some()
    );
    assert!(daemon.transfer_started_at.is_some());
    daemon.apply_network_event(NetworkEvent::Progress {
        medium: String::from("wifi_lan"),
        share_id,
        transferred_bytes: 4,
    });
    assert!(daemon.transfer_started_at.is_some());
}

#[test]
fn already_visible_pinned_peer_starts_after_payload_is_ready() {
    let mut daemon = Daemon::new();
    daemon.config.pinned_peer_id = Some(String::from(INBOUND_PEER_ID));
    daemon
        .sharing
        .observe_peer(INBOUND_PEER_ID, INBOUND_PEER_NAME);
    assert!(daemon.sharing.pin_peer(INBOUND_PEER_ID));
    daemon.outbound.remember_peer(
        INBOUND_PEER_ID,
        PeerRoute::Lan(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234)),
    );
    let share_id = daemon.queue_attachment(Attachment::text("hello"));
    assert_eq!(active_phase(&daemon), Phase::AwaitingPeerConsent);
    daemon
        .outbound
        .remember_text(share_id, String::from("hello"));
    assert!(daemon.start_pinned_outbound(share_id));
    assert_eq!(active_phase(&daemon), Phase::AwaitingPeerConsent);
    assert!(
        daemon
            .outbound
            .transfer(share_id, INBOUND_PEER_ID)
            .is_some()
    );
}

#[test]
fn pinned_peer_sighting_auto_starts_queued_outbound_share() {
    let mut daemon = Daemon::new();
    daemon.config.pinned_peer_id = Some(String::from(INBOUND_PEER_ID));
    let share_id = daemon.sharing.queue_outbound(Attachment::text("hello"));
    assert_eq!(active_phase(&daemon), Phase::WaitingForPeer);
    daemon.apply_network_event(NetworkEvent::PeerSeen {
        name: String::from(INBOUND_PEER_NAME),
        peer_id: String::from(INBOUND_PEER_ID),
        route: PeerRoute::Lan(SocketAddrV4::new(
            Ipv4Addr::new(127, 0, 0, 1),
            1234,
        )),
    });
    let share = daemon
        .sharing
        .snapshot()
        .active_share()
        .expect("queued share");
    assert_eq!(share.id().get(), share_id.get());
    assert_eq!(share.phase(), Phase::AwaitingPeerConsent);
    assert_eq!(share.id_string(), share_id.get().to_string());
}

#[test]
fn peer_lost_removes_the_visible_candidate() {
    let mut daemon = Daemon::new();
    daemon.apply_network_event(NetworkEvent::PeerSeen {
        name: String::from(INBOUND_PEER_NAME),
        peer_id: String::from(INBOUND_PEER_ID),
        route: PeerRoute::Lan(SocketAddrV4::new(
            Ipv4Addr::new(127, 0, 0, 1),
            9,
        )),
    });
    assert!(
        daemon
            .sharing
            .snapshot()
            .peers()
            .iter()
            .any(|peer| peer.id() == INBOUND_PEER_ID)
    );
    daemon.apply_network_event(NetworkEvent::PeerLost {
        peer_id: String::from(INBOUND_PEER_ID),
    });
    assert!(
        daemon
            .sharing
            .snapshot()
            .peers()
            .iter()
            .all(|peer| peer.id() != INBOUND_PEER_ID)
    );
}
