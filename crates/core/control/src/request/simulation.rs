use super::{Envelope, Request};
use crate::PROTOCOL_VERSION;

impl Envelope {
    /// Creates an explicit completed-transfer event for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_complete(share_id: u64) -> Self {
        Self {
            request: Request::SimulateComplete { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic discovery expiry for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_discovery_timeout() -> Self {
        Self {
            request: Request::SimulateDiscoveryTimeout,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic transfer failure for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_fail(share_id: u64) -> Self {
        Self {
            request: Request::SimulateFail { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic inbound file offer for local testing.
    #[must_use]
    #[inline]
    pub fn simulate_incoming_file(name: &str, size_bytes: u64) -> Self {
        Self {
            request: Request::SimulateIncomingFile {
                name: String::from(name),
                size_bytes,
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic inbound text event for local testing.
    #[must_use]
    #[inline]
    pub fn simulate_incoming_text(text: &str) -> Self {
        Self {
            request: Request::SimulateIncomingText {
                text: String::from(text),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic inbound URL offer for local testing.
    #[must_use]
    #[inline]
    pub fn simulate_incoming_url(url: &str) -> Self {
        Self {
            request: Request::SimulateIncomingUrl {
                url: String::from(url),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic peer-consent event for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_peer_accept(share_id: u64) -> Self {
        Self {
            request: Request::SimulatePeerAccept { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic peer-loss event for local testing.
    #[must_use]
    #[inline]
    pub fn simulate_peer_lost(peer_id: &str) -> Self {
        Self {
            request: Request::SimulatePeerLost {
                peer_id: String::from(peer_id),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic peer rejection for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_peer_reject(share_id: u64) -> Self {
        Self {
            request: Request::SimulatePeerReject { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic peer-discovery event for local testing.
    #[must_use]
    #[inline]
    pub fn simulate_peer_seen(peer_id: &str, name: &str) -> Self {
        Self {
            request: Request::SimulatePeerSeen {
                name: String::from(name),
                peer_id: String::from(peer_id),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a deterministic progress event for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_progress(
        share_id: u64,
        transferred_bytes: u64,
    ) -> Self {
        Self {
            request: Request::SimulateProgress {
                share_id,
                transferred_bytes,
            },
            version: PROTOCOL_VERSION,
        }
    }
}
