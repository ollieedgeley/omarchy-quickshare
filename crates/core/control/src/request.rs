/// Deterministic control requests used by the simulator.
mod simulation;
/// Submission constructors with optional peer selection.
mod submission;

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};

use crate::PROTOCOL_VERSION;

/// One versioned command sent to the local endpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    /// The command carried by this envelope.
    request: Request,
    /// Caller identity for submit deduplication; absent on other commands.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_request_id"
    )]
    request_id: Option<String>,
    /// The protocol version used to encode the command.
    version: u16,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Simulation and submission constructors own request submodules"
)]
impl Envelope {
    /// Creates a request to accept one inbound share.
    #[must_use]
    #[inline]
    pub const fn accept(share_id: u64) -> Self {
        Self {
            request: Request::Accept { share_id },
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to cancel one active share.
    #[must_use]
    #[inline]
    pub const fn cancel(share_id: u64) -> Self {
        Self {
            request: Request::Cancel { share_id },
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to close inbound discoverability.
    #[must_use]
    #[inline]
    pub const fn close_visibility() -> Self {
        Self {
            request: Request::CloseVisibility,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to start or restart outbound peer discovery.
    #[must_use]
    #[inline]
    pub const fn discover() -> Self {
        Self {
            request: Request::Discover,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to clear one terminal share from public state.
    #[must_use]
    #[inline]
    pub const fn dismiss(share_id: u64) -> Self {
        Self {
            request: Request::Dismiss { share_id },
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Requests ten minutes of inbound permission when durable policy is off.
    #[must_use]
    #[inline]
    pub const fn open_visibility() -> Self {
        Self {
            request: Request::OpenVisibility,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Queries a submission without submitting or retrying it.
    #[must_use]
    #[inline]
    pub fn operation(request_id: &str) -> Self {
        Self {
            request: Request::Operation {
                request_id: String::from(request_id),
            },
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to persist and activate one preference.
    #[must_use]
    #[inline]
    pub fn patch_preferences(key: &str, value: &str) -> Self {
        Self {
            request: Request::PatchPreferences {
                key: String::from(key),
                value: String::from(value),
            },
            request_id: None,
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
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to reject one inbound share.
    #[must_use]
    #[inline]
    pub const fn reject(share_id: u64) -> Self {
        Self {
            request: Request::Reject { share_id },
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Returns the command carried by this envelope.
    #[must_use]
    #[inline]
    pub const fn request(&self) -> &Request {
        &self.request
    }

    /// Returns the submission identity supplied by the caller.
    #[must_use]
    #[inline]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
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
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request for the endpoint's public state.
    #[must_use]
    #[inline]
    pub const fn snapshot() -> Self {
        Self {
            request: Request::Snapshot,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to check the local endpoint's readiness.
    #[must_use]
    #[inline]
    pub const fn status() -> Self {
        Self {
            request: Request::Status,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to stop outbound peer discovery.
    #[must_use]
    #[inline]
    pub const fn stop_discovery() -> Self {
        Self {
            request: Request::StopDiscovery,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Creates a request to clear the single pinned peer.
    #[must_use]
    #[inline]
    pub const fn unpin_peer() -> Self {
        Self {
            request: Request::UnpinPeer,
            request_id: None,
            version: PROTOCOL_VERSION,
        }
    }

    /// Checks identities appear only where the protocol permits.
    #[expect(
        clippy::ref_patterns,
        reason = "Borrow query IDs without copying or implicit ref patterns"
    )]
    pub(crate) fn validate(&self) -> io::Result<()> {
        let valid_id = |id: &str| !id.is_empty() && id.len() <= 128;
        let valid = if matches!(
            self.request,
            Request::SubmitFile { .. }
                | Request::SubmitText { .. }
                | Request::SubmitUrl { .. }
        ) {
            self.request_id().is_some_and(valid_id)
        } else if let Request::Operation { ref request_id } = self.request {
            self.request_id.is_none() && valid_id(request_id)
        } else {
            self.request_id.is_none()
        };
        if valid {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or forbidden control request_id",
            ))
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
    /// Request ten minutes of inbound permission, without extending a lease.
    OpenVisibility,
    /// Reconcile a submission by its original caller identity.
    Operation {
        /// Opaque identity supplied on the original submit envelope.
        request_id: String,
    },
    /// Persist and activate one preference.
    PatchPreferences {
        /// Preference name in the saved document.
        key: String,
        /// Unparsed preference value supplied by the client.
        value: String,
    },
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
    /// Finalizes a fully transferred share in a simulated endpoint.
    SimulateComplete {
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

/// Rejects explicit null identities while allowing an omitted envelope field.
fn deserialize_request_id<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    String::deserialize(deserializer).map(Some)
}
