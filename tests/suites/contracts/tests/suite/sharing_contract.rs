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
}
