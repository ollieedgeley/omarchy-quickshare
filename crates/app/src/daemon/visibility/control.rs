//! Local-control admission and snapshot projection of shared visibility.

use alloc::sync::Arc;
use core::time::Duration;

use quickshare_sharing::{Attachment, Phase};

use super::{ConsentOutcome, PendingConsent, Resources};
use crate::daemon::Daemon;

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Visibility controls own their admission boundary"
)]
impl Daemon {
    pub(crate) fn activate_visibility(&self, generation: u64) {
        if let Some(network) = &self.network {
            if let Err(error) = network.open_visibility(generation) {
                self.visibility
                    .complete_activation(generation, Err(error.to_string()));
            }
        } else {
            self.visibility
                .complete_activation(generation, Ok(Resources::default()));
        }
    }

    pub(crate) fn sync_visibility(&mut self) {
        let (state, status) = self.visibility.status();
        self.sharing.set_visibility_status(state, status);
        let Some(pending) = self.pending_consent.as_ref() else {
            return;
        };
        let Some(share_id) = pending.share_id() else {
            return;
        };
        match pending.outcome() {
            ConsentOutcome::TimedOut => {
                if self.sharing.fail(share_id) {
                    let _observed = self.sharing.record_observation(
                        share_id,
                        None,
                        None,
                        Some("timed_out"),
                        Some(crate::daemon::observations::recovery_guidance(
                            "timed_out",
                        )),
                    );
                }
            }
            ConsentOutcome::Cancelled => {
                let _cancelled = self.sharing.cancel(share_id);
            }
            _ => {}
        }
        if !self.sharing.snapshot().active_share().is_some_and(|share| {
            share.id().get() == share_id
                && matches!(
                    share.phase(),
                    Phase::AwaitingLocalConsent | Phase::Transferring
                )
        }) {
            self.pending_consent = None;
        }
    }

    pub(crate) fn admit_inbound(
        &mut self,
        attachment: Attachment,
        peer_id: &str,
        size_bytes: Option<u64>,
        consent: Arc<PendingConsent>,
    ) -> Option<u64> {
        if self.sharing.snapshot().active_share().is_some_and(|share| {
            matches!(
                share.phase(),
                Phase::WaitingForPeer
                    | Phase::AwaitingLocalConsent
                    | Phase::AwaitingPeerConsent
                    | Phase::Transferring
            )
        }) {
            let _rejected = consent.reject(0);
            return None;
        }
        if !self.visibility.admit(&consent) {
            return None;
        }
        let Some(share_id) = self
            .sharing
            .offer_inbound_sized(attachment, peer_id, size_bytes)
        else {
            let _rejected = consent.reject(0);
            return None;
        };
        consent.bind(share_id.get());
        self.pending_consent = Some(consent);
        Some(share_id.get())
    }

    pub(crate) fn accept_pending(&mut self, share_id: u64) -> bool {
        let Some(pending) = self.pending_consent.as_ref() else {
            return false;
        };
        if !self.sharing.snapshot().active_share().is_some_and(|share| {
            share.id().get() == share_id
                && share.phase() == Phase::AwaitingLocalConsent
        }) || !pending.accept(share_id)
        {
            self.sync_visibility();
            return false;
        }
        self.sharing.accept_inbound(share_id)
    }

    pub(crate) fn reject_pending(&mut self, share_id: u64) -> bool {
        if let Some(pending) = self.pending_consent.as_ref()
            && pending.share_id() == Some(share_id)
            && pending.reject(share_id)
        {
            return self.sharing.reject_inbound(share_id);
        }
        self.sync_visibility();
        false
    }

    pub(crate) fn simulate_inbound(&mut self, attachment: Attachment) -> bool {
        let Some(generation) = self.visibility.generation_if_open() else {
            return false;
        };
        let Some(consent) = self.visibility.propose(
            generation,
            Duration::from_secs(self.config.consent_timeout_secs),
        ) else {
            return false;
        };
        self.admit_inbound(attachment, "pixel-8", None, consent)
            .is_some()
    }
}
