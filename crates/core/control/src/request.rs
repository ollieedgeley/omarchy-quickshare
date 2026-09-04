use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::PROTOCOL_VERSION;

/// One versioned command sent to the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The command carried by this envelope.
    request: Request,
    /// The protocol version used to encode the command.
    version: u16,
}

impl Envelope {
    /// Creates a request to accept one inbound share.
    #[must_use]
    #[inline]
    pub const fn accept(share_id: u64) -> Self {
        Self {
            request: Request::Accept { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to cancel one active share.
    #[must_use]
    #[inline]
    pub const fn cancel(share_id: u64) -> Self {
        Self {
            request: Request::Cancel { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to close inbound discoverability.
    #[must_use]
    #[inline]
    pub const fn close_visibility() -> Self {
        Self {
            request: Request::CloseVisibility,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to start or restart outbound peer discovery.
    #[must_use]
    #[inline]
    pub const fn discover() -> Self {
        Self {
            request: Request::Discover,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to clear one terminal share from public state.
    #[must_use]
    #[inline]
    pub const fn dismiss(share_id: u64) -> Self {
        Self {
            request: Request::Dismiss { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to open inbound discoverability.
    #[must_use]
    #[inline]
    pub const fn open_visibility() -> Self {
        Self {
            request: Request::OpenVisibility,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to prefer one observed peer for future shares.
    #[must_use]
    #[inline]
    pub fn pin_peer(peer_id: &str) -> Self {
        Self {
            request: Request::PinPeer {
                peer_id: String::from(peer_id),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to reject one inbound share.
    #[must_use]
    #[inline]
    pub const fn reject(share_id: u64) -> Self {
        Self {
            request: Request::Reject { share_id },
            version: PROTOCOL_VERSION,
        }
    }

    /// Returns the command carried by this envelope.
    #[must_use]
    #[inline]
    pub const fn request(&self) -> &Request {
        &self.request
    }

    /// Creates a request to select an outbound peer.
    #[must_use]
    #[inline]
    pub fn select_peer(share_id: u64, peer_id: &str) -> Self {
        Self {
            request: Request::SelectPeer {
                peer_id: String::from(peer_id),
                share_id,
            },
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

    /// Creates a request for the endpoint's public state.
    #[must_use]
    #[inline]
    pub const fn snapshot() -> Self {
        Self {
            request: Request::Snapshot,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to check the local endpoint's readiness.
    #[must_use]
    #[inline]
    pub const fn status() -> Self {
        Self {
            request: Request::Status,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to stop outbound peer discovery.
    #[must_use]
    #[inline]
    pub const fn stop_discovery() -> Self {
        Self {
            request: Request::StopDiscovery,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit one file for sharing.
    #[must_use]
    #[inline]
    pub fn submit_file(path: &Path) -> Self {
        Self::submit_file_for(path, None)
    }

    /// Builds one file submission with an optional selected peer.
    fn submit_file_for(path: &Path, peer_id: Option<&str>) -> Self {
        Self {
            request: Request::SubmitFile {
                path: path.to_path_buf(),
                peer_id: peer_id.map(String::from),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit one file to a selected peer.
    #[must_use]
    #[inline]
    pub fn submit_file_to_peer(path: &Path, peer_id: &str) -> Self {
        Self::submit_file_for(path, Some(peer_id))
    }

    /// Creates a request to submit plain text for sharing.
    #[must_use]
    #[inline]
    pub fn submit_text(text: &str) -> Self {
        Self::submit_text_for(text, None)
    }

    /// Builds one text submission with an optional selected peer.
    fn submit_text_for(text: &str, peer_id: Option<&str>) -> Self {
        Self {
            request: Request::SubmitText {
                peer_id: peer_id.map(String::from),
                text: String::from(text),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit plain text to a selected peer.
    #[must_use]
    #[inline]
    pub fn submit_text_to_peer(text: &str, peer_id: &str) -> Self {
        Self::submit_text_for(text, Some(peer_id))
    }

    /// Creates a request to submit a URL for sharing.
    #[must_use]
    #[inline]
    pub fn submit_url(url: &str) -> Self {
        Self::submit_url_for(url, None)
    }

    /// Builds one URL submission with an optional selected peer.
    fn submit_url_for(url: &str, peer_id: Option<&str>) -> Self {
        Self {
            request: Request::SubmitUrl {
                peer_id: peer_id.map(String::from),
                url: String::from(url),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit a URL to a selected peer.
    #[must_use]
    #[inline]
    pub fn submit_url_to_peer(url: &str, peer_id: &str) -> Self {
        Self::submit_url_for(url, Some(peer_id))
    }

    /// Creates a request to clear the single pinned peer.
    #[must_use]
    #[inline]
    pub const fn unpin_peer() -> Self {
        Self {
            request: Request::UnpinPeer,
            version: PROTOCOL_VERSION,
        }
    }

    /// Returns the protocol version used by this command.
    #[must_use]
    #[inline]
    pub const fn version(&self) -> u16 {
        self.version
    }
}

/// A command supported by the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "type")]
#[non_exhaustive]
pub enum Request {
    /// Accept an inbound share after showing it to the local user.
    Accept {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Cancel an active share by its local identifier.
    Cancel {
        /// Identifier returned when the share was queued.
        share_id: u64,
    },
    /// Close inbound discoverability.
    CloseVisibility,
    /// Start or restart outbound peer discovery.
    Discover,
    /// Clear one terminal share after its result has been observed.
    Dismiss {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Open inbound discoverability.
    OpenVisibility,
    /// Prefer one observed peer for future outbound shares.
    PinPeer {
        /// Stable identifier advertised by the peer.
        peer_id: String,
    },
    /// Reject an inbound share after showing it to the local user.
    Reject {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Select an observed peer for one outbound share.
    SelectPeer {
        /// Stable identifier advertised by the peer.
        peer_id: String,
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Expire one running search on a simulated endpoint.
    SimulateDiscoveryTimeout,
    /// Inject a transfer failure into a simulated endpoint.
    SimulateFail {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Inject an inbound file offer into a simulated endpoint.
    SimulateIncomingFile {
        /// User-visible filename advertised by the peer.
        name: String,
        /// Declared attachment size.
        size_bytes: u64,
    },
    /// Inject an inbound text offer into a simulated endpoint.
    SimulateIncomingText {
        /// Exact text offered by the simulated peer.
        text: String,
    },
    /// Inject an inbound URL offer into a simulated endpoint.
    SimulateIncomingUrl {
        /// Exact URL offered by the simulated peer.
        url: String,
    },
    /// Inject peer acceptance into a simulated endpoint.
    SimulatePeerAccept {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Remove one peer from a simulated endpoint's discovery results.
    SimulatePeerLost {
        /// Stable identifier advertised by the peer.
        peer_id: String,
    },
    /// Inject peer rejection into a simulated endpoint.
    SimulatePeerReject {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Add or refresh one simulated peer in discovery results.
    SimulatePeerSeen {
        /// User-visible device name.
        name: String,
        /// Stable identifier advertised by the peer.
        peer_id: String,
    },
    /// Inject payload progress into a simulated endpoint.
    SimulateProgress {
        /// Stable local share identifier.
        share_id: u64,
        /// Total bytes observed at the transfer seam.
        transferred_bytes: u64,
    },
    /// Read the endpoint's current public state.
    Snapshot,
    /// Check whether the local endpoint can accept commands.
    Status,
    /// Stop outbound peer discovery.
    StopDiscovery,
    /// Submit one file for an outbound share.
    SubmitFile {
        /// The path to the file on the local machine.
        path: PathBuf,
        /// Peer selected atomically with this submission.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer_id: Option<String>,
    },
    /// Submit plain text for an outbound share.
    SubmitText {
        /// Peer selected atomically with this submission.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer_id: Option<String>,
        /// The exact text supplied by the user.
        text: String,
    },
    /// Submit a URL for an outbound share.
    SubmitUrl {
        /// Peer selected atomically with this submission.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        peer_id: Option<String>,
        /// The exact URL supplied by the user.
        url: String,
    },
    /// Clear the single preferred outbound peer.
    UnpinPeer,
}
