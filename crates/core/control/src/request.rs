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

    /// Creates a deterministic peer-consent event for local testing.
    #[must_use]
    #[inline]
    pub const fn simulate_peer_accept(share_id: u64) -> Self {
        Self {
            request: Request::SimulatePeerAccept { share_id },
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

    /// Creates a request to submit one file for sharing.
    #[must_use]
    #[inline]
    pub fn submit_file(path: &Path) -> Self {
        Self {
            request: Request::SubmitFile {
                path: path.to_path_buf(),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit plain text for sharing.
    #[must_use]
    #[inline]
    pub fn submit_text(text: &str) -> Self {
        Self {
            request: Request::SubmitText {
                text: String::from(text),
            },
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to submit a URL for sharing.
    #[must_use]
    #[inline]
    pub fn submit_url(url: &str) -> Self {
        Self {
            request: Request::SubmitUrl {
                url: String::from(url),
            },
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
    /// Inject an inbound text offer into a simulated endpoint.
    SimulateIncomingText {
        /// Exact text offered by the simulated peer.
        text: String,
    },
    /// Inject peer acceptance into a simulated endpoint.
    SimulatePeerAccept {
        /// Stable local share identifier.
        share_id: u64,
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
    /// Submit one file for an outbound share.
    SubmitFile {
        /// The path to the file on the local machine.
        path: PathBuf,
    },
    /// Submit plain text for an outbound share.
    SubmitText {
        /// The exact text supplied by the user.
        text: String,
    },
    /// Submit a URL for an outbound share.
    SubmitUrl {
        /// The exact URL supplied by the user.
        url: String,
    },
}
