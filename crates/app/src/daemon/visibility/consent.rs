//! One offer's admission, decision, and independent boot-clock deadline.

use core::time::Duration;
use std::sync::{Mutex, MutexGuard};

use super::clock::Clock;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConsentOutcome {
    Offered,
    Pending,
    Accepted(u64),
    Rejected(u64),
    Cancelled,
    TimedOut,
}

#[derive(Debug)]
struct Decision {
    outcome: ConsentOutcome,
    share_id: Option<u64>,
}

#[derive(Debug)]
pub(crate) struct PendingConsent {
    generation: u64,
    clock: Clock,
    deadline: Duration,
    decision: Mutex<Decision>,
}

impl PendingConsent {
    pub(super) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn new(
        generation: u64,
        clock: Clock,
        timeout: Duration,
    ) -> Self {
        let deadline = clock.now().saturating_add(timeout);
        Self {
            generation,
            clock,
            deadline,
            decision: Mutex::new(Decision {
                outcome: ConsentOutcome::Offered,
                share_id: None,
            }),
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "Poisoned consent must not recover an uncertain decision"
    )]
    fn decision(&self) -> MutexGuard<'_, Decision> {
        let mut decision =
            self.decision.lock().expect("consent state poisoned");
        if matches!(
            decision.outcome,
            ConsentOutcome::Offered | ConsentOutcome::Pending
        ) && self.clock.now() >= self.deadline
        {
            decision.outcome = ConsentOutcome::TimedOut;
        }
        decision
    }

    pub(super) fn admit(&self) -> bool {
        let mut decision = self.decision();
        if decision.outcome != ConsentOutcome::Offered {
            return false;
        }
        decision.outcome = ConsentOutcome::Pending;
        true
    }

    pub(crate) fn bind(&self, share_id: u64) {
        let mut decision = self.decision();
        decision.share_id = Some(share_id);
    }

    pub(crate) fn share_id(&self) -> Option<u64> {
        self.decision().share_id
    }

    pub(crate) fn outcome(&self) -> ConsentOutcome {
        self.decision().outcome
    }

    pub(crate) fn accept(&self, share_id: u64) -> bool {
        let mut decision = self.decision();
        if decision.outcome != ConsentOutcome::Pending
            || decision.share_id != Some(share_id)
        {
            return false;
        }
        decision.outcome = ConsentOutcome::Accepted(share_id);
        true
    }

    pub(crate) fn reject(&self, share_id: u64) -> bool {
        let mut decision = self.decision();
        if !matches!(
            decision.outcome,
            ConsentOutcome::Offered | ConsentOutcome::Pending
        ) {
            return false;
        }
        decision.outcome = ConsentOutcome::Rejected(share_id);
        true
    }

    pub(crate) fn cancel_unadmitted(&self) {
        let mut decision = self.decision();
        if decision.outcome == ConsentOutcome::Offered {
            decision.outcome = ConsentOutcome::Cancelled;
        }
    }

    pub(crate) fn cancel(&self) {
        let mut decision = self.decision();
        if matches!(
            decision.outcome,
            ConsentOutcome::Offered | ConsentOutcome::Pending
        ) {
            decision.outcome = ConsentOutcome::Cancelled;
        }
    }
}
