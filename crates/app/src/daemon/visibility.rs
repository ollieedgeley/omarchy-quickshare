//! Daemon-owned permission, activation, resources, and inbound admission.

pub(crate) mod clock;
mod consent;
mod control;
pub(crate) mod resources;

#[cfg(test)]
pub(crate) mod tests;

pub(crate) use consent::{ConsentOutcome, PendingConsent};
pub(crate) use resources::Resources;

use alloc::sync::{Arc, Weak};
use core::time::Duration;
use std::sync::Mutex;

use quickshare_sharing::{VisibilityState, VisibilityStatus};

use self::clock::Clock;

const TEMPORARY_LIFETIME: Duration = Duration::from_secs(600);

#[derive(Debug)]
struct State {
    generation: u64,
    policy: bool,
    requested: bool,
    phase: VisibilityState,
    deadline: Option<Duration>,
    resources: Option<Resources>,
    media: Vec<String>,
    error: Option<String>,
    pending: Weak<PendingConsent>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            generation: 0,
            policy: false,
            requested: false,
            phase: VisibilityState::Closed,
            deadline: None,
            resources: None,
            media: Vec::new(),
            error: None,
            pending: Weak::new(),
        }
    }
}

impl State {
    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "Generation exhaustion must not reuse admission tokens"
    )]
    fn revoke(&mut self, explicit: bool) -> Option<Resources> {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("visibility generation exhausted");
        self.requested = false;
        self.phase = VisibilityState::Closed;
        self.deadline = None;
        self.media.clear();
        self.error = None;
        if explicit && let Some(pending) = self.pending.upgrade() {
            pending.cancel();
        }
        self.resources.take()
    }

    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "Generation exhaustion must not reuse admission tokens"
    )]
    fn request(&mut self) -> Option<u64> {
        if self.requested {
            return None;
        }
        self.generation = self
            .generation
            .checked_add(1)
            .expect("visibility generation exhausted");
        self.requested = true;
        self.phase = VisibilityState::Starting;
        self.deadline = None;
        self.error = None;
        Some(self.generation)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Visibility {
    state: Arc<Mutex<State>>,
    clock: Clock,
}

impl Visibility {
    #[expect(
        clippy::expect_used,
        reason = "Poisoned permission state must fail closed"
    )]
    fn update<T>(&self, action: impl FnOnce(&mut State, Duration) -> T) -> T {
        let (result, expired, generation) = {
            let mut state =
                self.state.lock().expect("visibility state poisoned");
            let now = self.clock.now();
            let expired =
                if state.deadline.is_some_and(|deadline| now >= deadline) {
                    state.revoke(false)
                } else {
                    None
                };
            let result = action(&mut state, now);
            (result, expired, state.generation)
        };
        self.release(expired, generation);
        result
    }

    #[expect(
        clippy::expect_used,
        reason = "Poisoned permission state must fail closed"
    )]
    fn release(&self, resources: Option<Resources>, generation: u64) {
        if let Some(resources) = resources
            && let Err(error) = resources.close()
        {
            let mut state =
                self.state.lock().expect("visibility state poisoned");
            if state.generation == generation {
                state.error = Some(error);
            }
        }
    }

    pub(crate) fn request_open(&self) -> Option<u64> {
        self.update(|state, _| state.request())
    }

    pub(crate) fn set_policy(&self, policy: bool) -> Option<u64> {
        let (activation, released, generation) = self.update(|state, _| {
            if state.policy == policy {
                return (None, None, state.generation);
            }
            state.policy = policy;
            if policy {
                state.deadline = None;
                (state.request(), None, state.generation)
            } else {
                let released = state.revoke(true);
                (None, released, state.generation)
            }
        });
        self.release(released, generation);
        activation
    }

    pub(crate) fn close(&self) {
        let (resources, generation) = self.update(|state, _| {
            let resources = state.revoke(true);
            (resources, state.generation)
        });
        self.release(resources, generation);
    }

    pub(crate) fn complete_activation(
        &self,
        generation: u64,
        result: Result<Resources, String>,
    ) {
        let unused = self.update(|state, now| {
            if state.generation != generation
                || !state.requested
                || state.phase != VisibilityState::Starting
            {
                return result.ok();
            }
            match result {
                Ok(resources) => {
                    state.media = resources.media();
                    state.resources = Some(resources);
                    state.phase = VisibilityState::Open;
                    state.deadline = (!state.policy)
                        .then(|| now.saturating_add(TEMPORARY_LIFETIME));
                }
                Err(error) => {
                    state.requested = false;
                    state.phase = VisibilityState::Unavailable;
                    state.error = Some(error);
                }
            }
            None
        });
        self.release(unused, generation);
    }

    pub(crate) fn activation_requested(&self, generation: u64) -> bool {
        self.update(|state, _| {
            state.generation == generation
                && state.requested
                && state.phase == VisibilityState::Starting
        })
    }

    pub(crate) fn generation_if_open(&self) -> Option<u64> {
        self.update(|state, _| {
            (state.phase == VisibilityState::Open && state.requested)
                .then_some(state.generation)
        })
    }

    pub(crate) fn with_resources<T>(
        &self,
        action: impl FnOnce(&mut Resources) -> T,
    ) -> Option<T> {
        self.update(|state, _| state.resources.as_mut().map(action))
    }

    pub(crate) fn propose(
        &self,
        generation: u64,
        timeout: Duration,
    ) -> Option<Arc<PendingConsent>> {
        self.update(|state, _| {
            (state.phase == VisibilityState::Open
                && state.requested
                && state.generation == generation)
                .then(|| {
                    Arc::new(PendingConsent::new(
                        generation,
                        self.clock.clone(),
                        timeout,
                    ))
                })
        })
    }

    pub(crate) fn admit(&self, pending: &Arc<PendingConsent>) -> bool {
        self.update(|state, _| {
            if state.phase != VisibilityState::Open
                || !state.requested
                || state.generation != pending.generation()
                || state.pending.upgrade().is_some_and(|prior| {
                    matches!(
                        prior.outcome(),
                        ConsentOutcome::Pending | ConsentOutcome::Accepted(_)
                    )
                })
            {
                let _rejected = pending.reject(0);
                return false;
            }
            if !pending.admit() {
                return false;
            }
            state.pending = Arc::downgrade(pending);
            true
        })
    }

    pub(crate) fn status(&self) -> (VisibilityState, VisibilityStatus) {
        self.update(|state, now| {
            let remaining_secs = state.deadline.map(|deadline| {
                let remaining = deadline.saturating_sub(now);
                remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0)
            });
            (
                state.phase,
                VisibilityStatus {
                    discoverable: state.policy,
                    requested: state.requested,
                    temporary: state.requested && !state.policy,
                    remaining_secs,
                    available_media: state.media.clone(),
                    error: state.error.clone(),
                },
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn with_clock(clock: Clock) -> Self {
        Self {
            clock,
            ..Self::default()
        }
    }
}
