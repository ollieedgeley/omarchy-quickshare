use crate::attachment::Attachment;
use crate::peer::PeerSnapshot;

use super::{Direction, Phase, ShareId, ShareSnapshot};

impl ShareSnapshot {
    /// Returns the attachment offered by this share.
    #[must_use]
    pub const fn attachment(&self) -> &Attachment {
        &self.attachment
    }

    /// Returns the share direction relative to the local endpoint.
    #[must_use]
    pub const fn direction(&self) -> Direction {
        self.direction
    }

    /// Returns the stable local share identifier.
    #[must_use]
    pub const fn id(&self) -> ShareId {
        self.id
    }

    /// Returns the lossless decimal form of [`Self::id`].
    #[must_use]
    pub fn id_string(&self) -> &str {
        &self.id_string
    }

    /// Creates a visible active-share snapshot.
    #[must_use]
    pub(crate) fn new(
        attachment: Attachment,
        direction: Direction,
        id: ShareId,
        phase: Phase,
        total_bytes: u64,
    ) -> Self {
        Self {
            attachment,
            direction,
            id,
            id_string: id.get().to_string(),
            medium: None,
            peer: None,
            phase,
            recovery_guidance: None,
            remaining_seconds: None,
            terminal_reason: None,
            total_bytes,
            transferred_bytes: 0,
            verification_code: None,
        }
    }

    /// Returns the peer selected for this share.
    #[must_use]
    pub const fn peer(&self) -> Option<&PeerSnapshot> {
        self.peer.as_ref()
    }

    /// Returns the current lifecycle phase.
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// Returns the selected medium name, when known.
    #[must_use]
    pub fn medium(&self) -> Option<&str> {
        self.medium.as_deref()
    }

    /// Records monotonic transfer progress and completion.
    pub(crate) fn record_progress(&mut self, transferred_bytes: u64) -> bool {
        if self.phase != Phase::Transferring
            || transferred_bytes < self.transferred_bytes
            || transferred_bytes > self.total_bytes
        {
            return false;
        }
        self.transferred_bytes = transferred_bytes;
        if transferred_bytes == self.total_bytes {
            self.set_phase(Phase::Completed);
        }
        true
    }

    /// Stores one validated code while consent remains undecided.
    pub(crate) fn record_verification_code(&mut self, code: &str) -> bool {
        if !matches!(
            self.phase,
            Phase::AwaitingLocalConsent | Phase::AwaitingPeerConsent
        ) || code.len() != 4
            || !code.bytes().all(|byte| byte.is_ascii_digit())
        {
            return false;
        }
        self.verification_code = Some(String::from(code));
        true
    }

    /// Returns remaining transfer seconds estimated by the daemon.
    #[must_use]
    pub const fn remaining_seconds(&self) -> Option<u64> {
        self.remaining_seconds
    }

    /// Returns recovery guidance after a terminal share.
    #[must_use]
    pub fn recovery_guidance(&self) -> Option<&str> {
        self.recovery_guidance.as_deref()
    }

    /// Replaces the attachment after inbound text or URL bytes arrive.
    pub(crate) fn replace_attachment(&mut self, attachment: Attachment) {
        self.total_bytes = attachment.byte_len();
        self.attachment = attachment;
    }

    /// Sets the selected peer and lifecycle phase.
    pub(crate) fn select_peer(&mut self, peer: PeerSnapshot, phase: Phase) {
        self.peer = Some(peer);
        self.phase = phase;
    }

    /// Records daemon-owned transfer observations.
    pub(crate) fn set_observation(
        &mut self,
        medium: Option<&str>,
        remaining_seconds: Option<u64>,
        terminal_reason: Option<&str>,
        recovery_guidance: Option<&str>,
    ) {
        if let Some(value) = medium {
            self.medium = Some(String::from(value));
        }
        self.remaining_seconds = remaining_seconds;
        if let Some(value) = terminal_reason {
            self.terminal_reason = Some(String::from(value));
        }
        if let Some(value) = recovery_guidance {
            self.recovery_guidance = Some(String::from(value));
        }
    }

    /// Changes the lifecycle phase.
    pub(crate) fn set_phase(&mut self, phase: Phase) {
        self.phase = phase;
        if !matches!(
            phase,
            Phase::AwaitingLocalConsent | Phase::AwaitingPeerConsent
        ) {
            self.verification_code = None;
        }
    }

    /// Returns the stable terminal reason, when the share has ended.
    #[must_use]
    pub fn terminal_reason(&self) -> Option<&str> {
        self.terminal_reason.as_deref()
    }

    /// Returns total declared attachment bytes.
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Returns bytes observed across the transfer seam.
    #[must_use]
    pub const fn transferred_bytes(&self) -> u64 {
        self.transferred_bytes
    }

    /// Returns the UKEY2 code while this share awaits consent.
    #[must_use]
    pub fn verification_code(&self) -> Option<&str> {
        self.verification_code.as_deref()
    }
}
