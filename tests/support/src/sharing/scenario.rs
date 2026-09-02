//! Version one of the language-neutral transfer scenario model.

use serde::Deserialize;

/// The only scenario schema understood by this model.
const SCHEMA_VERSION: u8 = 1;
/// The encoded length of a SHA-256 digest.
const SHA256_HEX_LENGTH: usize = 64;

/// The direction of the share relative to the local endpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The local endpoint receives the attachment.
    Inbound,
    /// The local endpoint sends the attachment.
    Outbound,
}

/// The local endpoint's role in connection establishment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ConnectionRole {
    /// The local endpoint initiates the connection.
    Initiator,
    /// The local endpoint responds to the connection.
    Responder,
}

/// A connection medium admitted by the local verification matrix.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum Medium {
    /// Bluetooth Low Energy.
    Ble,
    /// Bluetooth Classic.
    BluetoothClassic,
    /// A temporary Wi-Fi hotspot.
    Hotspot,
    /// An existing local IP network.
    SameLan,
    /// A Wi-Fi Direct group.
    WifiDirect,
}

/// A required first-slice attachment family.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    /// File bytes and metadata.
    File,
    /// Plain text.
    Text,
    /// A URL.
    Url,
}

/// Stable facts used to verify an attachment without exposing its contents.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct AttachmentFacts {
    /// The semantic attachment family.
    pub kind: AttachmentKind,
    /// The lowercase SHA-256 digest of the payload.
    pub sha256: String,
    /// The declared payload size.
    pub size: u64,
}

/// The peer's consent response.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum PeerDecision {
    /// Continue with payload transfer.
    Accept,
    /// Reject before payload transfer.
    Reject,
}

/// The endpoint that cancels an active share.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum CancelActor {
    /// The share receiver cancels.
    Receiver,
    /// The share sender cancels.
    Sender,
}

/// A deterministic cancellation injected after a byte boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Cancellation {
    /// The endpoint that requests cancellation.
    pub actor: CancelActor,
    /// The observed payload bytes before cancellation.
    pub after_bytes: u64,
}

/// The terminal result visible through the transfer seam.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum TerminalOutcome {
    /// An endpoint cancelled the active share.
    Cancelled,
    /// The attachment was committed successfully.
    Completed,
    /// The peer rejected the offer.
    Rejected,
}

/// Public evidence expected from any scenario driver.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct ExpectedOutcome {
    /// Whether all owned resources were released.
    pub cleanup_complete: bool,
    /// The terminal transfer result.
    pub terminal: TerminalOutcome,
    /// The payload bytes observed before the terminal result.
    pub transferred_bytes: u64,
}

/// One semantic scenario executable by a fake or simulator adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[non_exhaustive]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Attachment facts used for integrity checks.
    pub attachment: AttachmentFacts,
    /// An optional deterministic cancellation.
    pub cancellation: Option<Cancellation>,
    /// The direction relative to the local endpoint.
    pub direction: Direction,
    /// The public evidence the driver must produce.
    pub expected: ExpectedOutcome,
    /// A stable, human-readable scenario identifier.
    pub id: String,
    /// The local connection-establishment role.
    pub local_role: ConnectionRole,
    /// The forced connection medium.
    pub medium: Medium,
    /// The peer's consent response.
    pub peer_decision: PeerDecision,
    /// The language-neutral scenario schema version.
    pub schema: u8,
}

/// Why a semantic scenario is internally inconsistent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ValidationError {
    /// Cancellation happens beyond the declared payload boundary.
    CancellationBeyondPayload,
    /// Successful or rejected scenarios report incomplete cleanup.
    IncompleteCleanup,
    /// The expected result contradicts the scripted peer behavior.
    InconsistentExpectedOutcome,
    /// The payload digest is not lowercase SHA-256 hexadecimal.
    InvalidPayloadDigest,
    /// The scenario has no stable identifier.
    MissingIdentifier,
    /// A rejection also tries to transfer or cancel payload bytes.
    RejectionHasPayloadActivity,
    /// The schema version is not implemented.
    UnsupportedSchema,
}

impl Scenario {
    /// Derives the public result from deterministic peer actions.
    #[inline]
    pub(crate) fn scripted_outcome(&self) -> ExpectedOutcome {
        if self.peer_decision == PeerDecision::Reject {
            return ExpectedOutcome {
                cleanup_complete: true,
                terminal: TerminalOutcome::Rejected,
                transferred_bytes: 0,
            };
        }
        if let Some(cancellation) = self.cancellation {
            return ExpectedOutcome {
                cleanup_complete: true,
                terminal: TerminalOutcome::Cancelled,
                transferred_bytes: cancellation.after_bytes,
            };
        }
        ExpectedOutcome {
            cleanup_complete: true,
            terminal: TerminalOutcome::Completed,
            transferred_bytes: self.attachment.size,
        }
    }

    /// Validates invariants shared by every scenario driver.
    ///
    /// # Errors
    ///
    /// Returns the first invalid or contradictory scenario field.
    #[inline]
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema != SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedSchema);
        }
        if self.id.is_empty() {
            return Err(ValidationError::MissingIdentifier);
        }
        let digest = &self.attachment.sha256;
        let valid_digest = digest.len() == SHA256_HEX_LENGTH
            && digest.bytes().all(|byte| {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            });
        if !valid_digest {
            return Err(ValidationError::InvalidPayloadDigest);
        }
        if self.cancellation.is_some_and(|cancellation| {
            cancellation.after_bytes > self.attachment.size
        }) {
            return Err(ValidationError::CancellationBeyondPayload);
        }
        if self.peer_decision == PeerDecision::Reject
            && (self.cancellation.is_some()
                || self.expected.transferred_bytes != 0)
        {
            return Err(ValidationError::RejectionHasPayloadActivity);
        }
        if self.expected != self.scripted_outcome() {
            return Err(ValidationError::InconsistentExpectedOutcome);
        }
        if !self.expected.cleanup_complete {
            return Err(ValidationError::IncompleteCleanup);
        }
        Ok(())
    }
}
