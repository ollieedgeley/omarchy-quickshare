#[cfg(test)]
mod tests {
    use quickshare_sharing::{Attachment, Coordinator, Direction, Phase};

    #[test]
    fn submitted_text_waits_for_an_outbound_peer() {
        let mut coordinator = Coordinator::new();

        let share_id = coordinator.queue_outbound(Attachment::text("hello"));

        assert_eq!(share_id.get(), 1);
        let snapshot = coordinator.snapshot();
        let active_result = snapshot.active_share();
        assert!(active_result.is_some(), "queued share is not visible");
        let Some(active) = active_result else {
            return;
        };
        assert_eq!(active.id(), share_id);
        assert_eq!(active.direction(), Direction::Outbound);
        assert_eq!(active.phase(), Phase::WaitingForPeer);
        assert_eq!(active.transferred_bytes(), 0);
        assert_eq!(active.total_bytes(), 5);
    }

    #[test]
    fn active_share_can_be_cancelled_by_its_identifier() {
        let mut coordinator = Coordinator::new();
        let share_id = coordinator.queue_outbound(Attachment::text("hello"));

        let cancelled = coordinator.cancel(share_id.get());

        assert!(cancelled, "active share was not cancelled");
        let active_result = coordinator.snapshot().active_share();
        assert!(active_result.is_some(), "cancelled share is not visible");
        let Some(active) = active_result else {
            return;
        };
        assert_eq!(active.phase(), Phase::Cancelled);
        assert_eq!(active.transferred_bytes(), 0);
    }

    #[test]
    fn unrelated_share_identifier_cannot_cancel_the_active_share() {
        let mut coordinator = Coordinator::new();
        let share_id = coordinator.queue_outbound(Attachment::text("hello"));

        let cancelled = coordinator.cancel(share_id.get() + 1);

        assert!(!cancelled, "unrelated share cancelled active work");
        let active_result = coordinator.snapshot().active_share();
        assert!(active_result.is_some(), "active share disappeared");
        let Some(active) = active_result else {
            return;
        };
        assert_eq!(active.phase(), Phase::WaitingForPeer);
    }

    #[test]
    fn outbound_share_completes_with_an_observed_peer() {
        let mut coordinator = Coordinator::new();
        coordinator.observe_peer("pixel-8", "Ollie's Pixel");
        let share_id = coordinator.queue_outbound(Attachment::text("hello"));

        assert!(coordinator.select_peer(share_id.get(), "pixel-8"));
        assert!(coordinator.accept_by_peer(share_id.get()));
        assert!(coordinator.record_progress(share_id.get(), 2));
        assert!(coordinator.record_progress(share_id.get(), 5));

        let snapshot = coordinator.snapshot();
        assert_eq!(snapshot.peers().len(), 1);
        let active_result = snapshot.active_share();
        assert!(active_result.is_some(), "completed share is not visible");
        let Some(active) = active_result else {
            return;
        };
        assert_eq!(active.phase(), Phase::Completed);
        assert_eq!(active.transferred_bytes(), 5);
        assert_eq!(
            active.peer().map(quickshare_sharing::PeerSnapshot::id),
            Some("pixel-8"),
        );
    }

    #[test]
    fn pinned_peer_is_the_only_automatic_outbound_target() {
        let mut coordinator = Coordinator::new();
        coordinator.observe_peer("pixel-8", "Ollie's Pixel");
        coordinator.observe_peer("galaxy-tab", "Galaxy Tab");

        assert!(coordinator.pin_peer("pixel-8"));
        assert!(coordinator.pin_peer("galaxy-tab"));
        let share_id = coordinator.queue_outbound(Attachment::text("hello"));

        let peers = coordinator.snapshot().peers();
        let pinned = peers
            .iter()
            .filter(|peer| peer.is_pinned())
            .map(quickshare_sharing::PeerSnapshot::id)
            .collect::<Vec<_>>();
        assert_eq!(pinned, ["galaxy-tab"]);
        let active_result = coordinator.snapshot().active_share();
        assert!(active_result.is_some(), "pinned share is not visible");
        let Some(active) = active_result else {
            return;
        };
        assert_eq!(active.id(), share_id);
        assert_eq!(active.phase(), Phase::AwaitingPeerConsent);
        assert_eq!(
            active.peer().map(quickshare_sharing::PeerSnapshot::id),
            Some("galaxy-tab"),
        );
    }

    #[test]
    fn inbound_share_waits_for_consent_then_completes() {
        let mut coordinator = Coordinator::new();
        coordinator.observe_peer("pixel-8", "Ollie's Pixel");

        let offer_result = coordinator
            .offer_inbound(Attachment::file("photo.jpg", 4), "pixel-8");
        assert!(offer_result.is_some(), "known peer offer was rejected");
        let Some(share_id) = offer_result else {
            return;
        };

        let offered_result = coordinator.snapshot().active_share();
        assert!(offered_result.is_some(), "inbound offer is not visible");
        let Some(offered) = offered_result else {
            return;
        };
        assert_eq!(offered.direction(), Direction::Inbound);
        assert_eq!(offered.phase(), Phase::AwaitingLocalConsent);
        assert!(coordinator.accept_inbound(share_id.get()));
        assert!(coordinator.record_progress(share_id.get(), 4));
        let completed_result = coordinator.snapshot().active_share();
        assert!(
            completed_result.is_some(),
            "completed inbound share is not visible",
        );
        let Some(completed) = completed_result else {
            return;
        };
        assert_eq!(completed.phase(), Phase::Completed);
    }

    #[test]
    fn either_side_can_reject_before_transfer() {
        let mut coordinator = Coordinator::new();
        coordinator.observe_peer("pixel-8", "Ollie's Pixel");
        let outbound = coordinator.queue_outbound(Attachment::text("hello"));
        assert!(coordinator.select_peer(outbound.get(), "pixel-8"));
        assert!(coordinator.reject_by_peer(outbound.get()));
        let outbound_result = coordinator.snapshot().active_share();
        assert!(
            outbound_result.is_some(),
            "rejected outbound share is not visible",
        );
        let Some(outbound_rejected) = outbound_result else {
            return;
        };
        assert_eq!(outbound_rejected.phase(), Phase::Rejected);

        let inbound_result =
            coordinator.offer_inbound(Attachment::text("hello"), "pixel-8");
        assert!(inbound_result.is_some(), "known peer offer was rejected");
        let Some(inbound) = inbound_result else {
            return;
        };
        assert!(coordinator.reject_inbound(inbound.get()));
        let rejected_result = coordinator.snapshot().active_share();
        assert!(
            rejected_result.is_some(),
            "rejected inbound share is not visible",
        );
        let Some(inbound_rejected) = rejected_result else {
            return;
        };
        assert_eq!(inbound_rejected.phase(), Phase::Rejected);
    }

    #[test]
    fn failed_transfer_can_be_dismissed_after_it_stays_visible() {
        let mut coordinator = Coordinator::new();
        coordinator.observe_peer("pixel-8", "Ollie's Pixel");
        let share_id = coordinator.queue_outbound(Attachment::text("hello"));
        assert!(coordinator.select_peer(share_id.get(), "pixel-8"));
        assert!(coordinator.accept_by_peer(share_id.get()));

        assert!(coordinator.fail(share_id.get()));
        let failed_result = coordinator.snapshot().active_share();
        assert!(failed_result.is_some(), "failed share disappeared");
        let Some(failed) = failed_result else {
            return;
        };
        assert_eq!(failed.phase(), Phase::Failed);
        assert!(coordinator.dismiss(share_id.get()));
        assert!(coordinator.snapshot().active_share().is_none());
    }

    #[test]
    fn visible_peers_can_appear_and_disappear_during_discovery() {
        let mut coordinator = Coordinator::new();
        coordinator.observe_peer("pixel-8", "Ollie's Pixel");
        coordinator.observe_peer("galaxy-tab", "Galaxy Tab");

        assert!(coordinator.remove_peer("pixel-8"));
        assert!(!coordinator.remove_peer("missing"));

        let peer_ids = coordinator
            .snapshot()
            .peers()
            .iter()
            .map(quickshare_sharing::PeerSnapshot::id)
            .collect::<Vec<_>>();
        assert_eq!(peer_ids, ["galaxy-tab"]);
    }
}
