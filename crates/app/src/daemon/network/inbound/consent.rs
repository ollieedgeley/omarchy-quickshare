use core::time::Duration;
use std::sync::mpsc::{Receiver, RecvTimeoutError};

use super::super::NetworkCommand;
use crate::daemon::visibility::{ConsentOutcome, PendingConsent};
use quickshare_sharing::ProtocolError;

const CONSENT_POLL: Duration = Duration::from_millis(50);

pub(super) enum Consent {
    Accepted(u64),
    Rejected(u64),
    Cancelled,
    TimedOut,
}

pub(super) fn wait_for_consent<PollControl>(
    commands: &Receiver<NetworkCommand>,
    consent: &PendingConsent,
    on_other: &mut dyn FnMut(NetworkCommand) -> bool,
    permitted: impl Fn() -> bool,
    mut poll_control: PollControl,
) -> Result<Consent, (String, Option<u64>)>
where
    PollControl: FnMut() -> Result<(), ProtocolError>,
{
    loop {
        match consent.outcome() {
            ConsentOutcome::Accepted(id) => return Ok(Consent::Accepted(id)),
            ConsentOutcome::Rejected(id) => return Ok(Consent::Rejected(id)),
            ConsentOutcome::Cancelled => return Ok(Consent::Cancelled),
            ConsentOutcome::TimedOut => return Ok(Consent::TimedOut),
            ConsentOutcome::Failed(reason) => {
                return Err((String::from(reason), consent.share_id()));
            }
            ConsentOutcome::Offered if !permitted() => {
                consent.cancel_unadmitted();
                continue;
            }
            ConsentOutcome::Offered | ConsentOutcome::Pending => {}
        }
        poll_control().map_err(|error| {
            if matches!(&error, ProtocolError::Cancelled) {
                consent.cancel();
            } else {
                consent.fail(error.reason());
            }
            (String::from(error.reason()), consent.share_id())
        })?;
        match commands.recv_timeout(CONSENT_POLL) {
            Ok(command) => {
                if !on_other(command) {
                    consent.fail("disconnected");
                    return Err((
                        String::from("disconnected"),
                        consent.share_id(),
                    ));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                consent.fail("disconnected");
                return Err((String::from("disconnected"), consent.share_id()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Consent, NetworkCommand, wait_for_consent};
    use crate::daemon::visibility::{PendingConsent, Resources, Visibility};
    use alloc::sync::Arc;
    use core::time::Duration;
    use std::sync::mpsc;

    fn pending(timeout: Duration) -> (Visibility, Arc<PendingConsent>) {
        let visibility = Visibility::default();
        let generation = visibility.request_open().expect("activation");
        visibility.complete_activation(generation, Ok(Resources::default()));
        let consent = visibility.propose(generation, timeout).expect("offer");
        assert!(visibility.admit(&consent));
        consent.bind(4);
        (visibility, consent)
    }

    #[test]
    fn stop_discovery_is_dispatched_before_token_acceptance() {
        let (_visibility, consent) = pending(Duration::from_secs(1));
        let (sender, receiver) = mpsc::channel();
        sender
            .send(NetworkCommand::StopDiscovery)
            .expect("queue stop");
        let result = wait_for_consent(
            &receiver,
            &consent,
            &mut |command| {
                assert!(matches!(command, NetworkCommand::StopDiscovery));
                assert!(consent.accept(4));
                true
            },
            || true,
            || Ok(()),
        )
        .expect("accepted");
        assert!(matches!(result, Consent::Accepted(4)));
    }

    #[test]
    fn peer_loss_before_terminal_delivery_preserves_inbound_failure() {
        use crate::config::Config;
        use crate::daemon::network::NetworkEvent;
        use crate::daemon::visibility::tests::Fixture;
        use quickshare_sharing::{OfferKind, Phase, ProtocolError};

        let mut fixture = Fixture::new(Config::default());
        let _opened = fixture.open();
        let visibility = fixture.visibility();
        let generation = visibility.generation_if_open().expect("open");
        let consent = visibility
            .propose(generation, Duration::from_secs(20))
            .expect("offer");
        fixture
            .events()
            .send(NetworkEvent::InboundOffered {
                kind: OfferKind::File,
                name: String::from("interrupted.txt"),
                size_bytes: 0x0010_0001,
                verification_code: String::from("6251"),
                consent: Arc::clone(&consent),
            })
            .expect("offer event");
        let offered = fixture.snapshot();
        let share_id = offered.active_share().expect("admitted").id().get();
        let (_sender, receiver) = mpsc::channel();
        let (reason, identified) = wait_for_consent(
            &receiver,
            &consent,
            &mut |_| true,
            || true,
            || Err(ProtocolError::Disconnected),
        )
        .err()
        .expect("peer loss");
        let before_delivery = fixture.snapshot();
        let pending = before_delivery.active_share().expect("pending event");
        assert_eq!(pending.phase(), Phase::AwaitingLocalConsent);
        assert_eq!(pending.terminal_reason(), None);
        assert!(matches!(
            fixture
                .command(quickshare_control::request::Envelope::accept(
                    share_id
                ))
                .response(),
            quickshare_control::response::Response::NotFound
        ));
        fixture
            .events()
            .send(NetworkEvent::InboundFailed {
                reason,
                share_id: identified,
                consent: Some(consent),
            })
            .expect("terminal event");
        let snapshot = fixture.snapshot();
        let share = snapshot.active_share().expect("failed share");
        assert_eq!(share.id().get(), share_id);
        assert_eq!(share.phase(), Phase::Failed);
        assert_eq!(share.terminal_reason(), Some("disconnected"));
        assert_eq!(share.transferred_bytes(), 0);
        assert_eq!(share.total_bytes(), 0x0010_0001);
    }
}
