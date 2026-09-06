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
            ConsentOutcome::Offered if !permitted() => {
                consent.cancel_unadmitted();
                continue;
            }
            ConsentOutcome::Offered | ConsentOutcome::Pending => {}
        }
        poll_control().map_err(|error| {
            consent.cancel();
            (String::from(error.reason()), consent.share_id())
        })?;
        match commands.recv_timeout(CONSENT_POLL) {
            Ok(command) => {
                if !on_other(command) {
                    consent.cancel();
                    return Err((
                        String::from("disconnected"),
                        consent.share_id(),
                    ));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                consent.cancel();
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
}
