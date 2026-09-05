use core::time::Duration;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Instant;

use quickshare_sharing::ProtocolError;

use super::super::NetworkCommand;

const CONSENT_POLL: Duration = Duration::from_millis(50);

pub(super) enum Consent {
    Accepted(u64),
    Rejected(u64),
    TimedOut,
}

pub(super) fn wait_for_consent<PollControl>(
    commands: &Receiver<NetworkCommand>,
    deadline: Duration,
    on_other: &mut dyn FnMut(NetworkCommand) -> bool,
    mut poll_control: PollControl,
) -> Result<Consent, (String, Option<u64>)>
where
    PollControl: FnMut() -> Result<(), ProtocolError>,
{
    let started = Instant::now();
    loop {
        poll_control().map_err(|error| (String::from(error.reason()), None))?;
        if started.elapsed() >= deadline {
            return Ok(Consent::TimedOut);
        }
        match commands.recv_timeout(CONSENT_POLL) {
            Ok(NetworkCommand::AcceptInbound { share_id }) => {
                return Ok(Consent::Accepted(share_id));
            }
            Ok(NetworkCommand::RejectInbound { share_id }) => {
                return Ok(Consent::Rejected(share_id));
            }
            Ok(command) => {
                let close = matches!(command, NetworkCommand::CloseVisibility);
                if !on_other(command) {
                    return Err((String::from("disconnected"), None));
                }
                if close {
                    return Err((String::from("cancelled"), None));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err((String::from("disconnected"), None));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Consent, NetworkCommand, wait_for_consent};
    use core::time::Duration;
    use std::sync::mpsc;

    #[test]
    fn stop_discovery_is_dispatched_before_accept() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(NetworkCommand::StopDiscovery)
            .expect("queue stop");
        sender
            .send(NetworkCommand::AcceptInbound { share_id: 4 })
            .expect("queue accept");
        let mut dispatched = 0_u8;
        let consent = wait_for_consent(
            &receiver,
            Duration::from_secs(1),
            &mut |command| {
                assert!(matches!(command, NetworkCommand::StopDiscovery));
                dispatched = dispatched.saturating_add(1);
                true
            },
            || Ok(()),
        )
        .expect("accepted");
        assert!(matches!(consent, Consent::Accepted(4)));
        assert_eq!(dispatched, 1);
    }

    #[test]
    fn close_visibility_is_dispatched_and_cancels() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(NetworkCommand::CloseVisibility)
            .expect("queue close");
        let mut dispatched = false;
        let result = wait_for_consent(
            &receiver,
            Duration::from_secs(1),
            &mut |command| {
                dispatched = matches!(command, NetworkCommand::CloseVisibility);
                true
            },
            || Ok(()),
        );
        assert!(dispatched);
        assert!(matches!(result, Err((reason, None)) if reason == "cancelled"));
    }

    #[test]
    fn timeout_is_terminal() {
        let (_sender, receiver) = mpsc::channel::<NetworkCommand>();
        let consent = wait_for_consent(
            &receiver,
            Duration::from_millis(0),
            &mut |_| true,
            || Ok(()),
        )
        .expect("timed out");
        assert!(matches!(consent, Consent::TimedOut));
    }
}
