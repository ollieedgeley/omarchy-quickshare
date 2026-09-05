use quickshare_connections::Error as ConnectionError;
use std::{error, fmt, io};

/// One account-free paired-key operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairingStep {
    /// Sending the local paired-key encryption frame.
    SendEncryption,
    /// Receiving and decoding the peer paired-key encryption frame.
    ReceiveEncryption,
    /// Sending the local paired-key result frame.
    SendResult,
    /// Receiving and decoding the peer paired-key result frame.
    ReceiveResult,
}

impl PairingStep {
    /// Returns the stable snake-case diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SendEncryption => "send_encryption",
            Self::ReceiveEncryption => "receive_encryption",
            Self::SendResult => "send_result",
            Self::ReceiveResult => "receive_result",
        }
    }
}

/// A Sharing protocol failure attributed to one paired-key operation.
#[derive(Debug)]
pub struct PairingError {
    step: PairingStep,
    source: ProtocolError,
}

impl PairingError {
    /// Attributes a protocol failure to the operation that returned it.
    #[inline(never)]
    pub(in crate::protocol) fn new(
        step: PairingStep,
        source: ProtocolError,
    ) -> Self {
        Self { step, source }
    }

    /// Returns the paired-key operation that failed.
    #[must_use]
    pub const fn step(&self) -> PairingStep {
        self.step
    }

    /// Borrows the underlying Sharing protocol failure.
    #[must_use]
    pub const fn source_error(&self) -> &ProtocolError {
        &self.source
    }

    /// Returns the underlying Sharing protocol failure.
    #[must_use]
    pub fn into_source(self) -> ProtocolError {
        self.source
    }
}

impl fmt::Display for PairingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "paired-key {} failed: {}",
            self.step.as_str(),
            self.source
        )
    }
}

impl error::Error for PairingError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Failures while interpreting Sharing messages or transfer events.
#[derive(Debug)]
pub enum ProtocolError {
    /// The encrypted Connections session failed.
    Connection(ConnectionError),
    /// A Sharing protobuf could not be decoded.
    Decode(prost::DecodeError),
    /// Reading or writing the streamed file failed.
    Io(io::Error),
    /// Either endpoint cancelled the active transfer.
    Cancelled,
    /// The endpoint-info bytes violate the supported layout.
    InvalidAdvertisement,
    /// The Nearby LAN instance bytes violate the supported layout.
    InvalidMdnsInstance,
    /// A Sharing frame has the wrong kind or required field.
    InvalidFrame,
    /// The introduction is not one supported file, text, URL, or app
    /// attachment.
    InvalidOffer(&'static str),
    /// The peer rejected the file introduction.
    Rejected,
    /// The declared payload does not match its introduction.
    InvalidPayload,
    /// The encrypted connection closed before a terminal Sharing result.
    Disconnected,
    /// The peer reported that the offer timed out.
    TimedOut,
    /// The peer cannot accept this attachment kind.
    Unsupported,
}

impl ProtocolError {
    /// Returns the stable privacy-safe failure class exposed to local clients.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        match self {
            Self::Connection(source) => match source {
                ConnectionError::Io(_) => "connection_io",
                ConnectionError::Wire(_) => "connection_wire",
                ConnectionError::FrameTooLarge => "connection_frame_too_large",
                ConnectionError::UnexpectedFrame => {
                    "connection_unexpected_frame"
                }
                ConnectionError::Rejected => "connection_rejected",
                ConnectionError::Handshake => "connection_handshake",
                ConnectionError::Crypto => "connection_crypto",
                ConnectionError::InvalidPayload => "connection_invalid_payload",
                _ => "connection_unknown",
            },
            Self::Decode(_) => "sharing_decode",
            Self::Io(_) => "sharing_io",
            Self::Cancelled => "cancelled",
            Self::InvalidAdvertisement => "invalid_advertisement",
            Self::InvalidMdnsInstance => "invalid_mdns_instance",
            Self::InvalidFrame => "invalid_frame",
            Self::InvalidOffer(_) => "invalid_offer",
            Self::Rejected => "rejected",
            Self::InvalidPayload => "invalid_payload",
            Self::Disconnected => "disconnected",
            Self::TimedOut => "timed_out",
            Self::Unsupported => "unsupported",
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => {
                write!(formatter, "Connections failed: {error}")
            }
            Self::Cancelled => {
                formatter.write_str("file transfer was cancelled")
            }
            Self::Disconnected => {
                formatter.write_str("peer disconnected during the share")
            }
            Self::Decode(error) => {
                write!(formatter, "Sharing protobuf failed: {error}")
            }
            Self::Io(error) => {
                write!(formatter, "file stream failed: {error}")
            }
            Self::InvalidAdvertisement => {
                formatter.write_str("invalid endpoint advertisement")
            }
            Self::InvalidMdnsInstance => {
                formatter.write_str("invalid Nearby LAN instance")
            }
            Self::InvalidFrame => {
                formatter.write_str("unexpected Sharing frame")
            }
            Self::InvalidOffer(reason) => {
                write!(formatter, "invalid file offer: {reason}")
            }
            Self::Rejected => {
                formatter.write_str("peer did not accept the file offer")
            }
            Self::TimedOut => formatter.write_str("share offer timed out"),
            Self::Unsupported => {
                formatter.write_str("peer does not support this attachment")
            }
            Self::InvalidPayload => formatter.write_str("invalid file payload"),
        }
    }
}

impl error::Error for ProtocolError {}

impl From<ConnectionError> for ProtocolError {
    fn from(error: ConnectionError) -> Self {
        Self::Connection(error)
    }
}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<prost::DecodeError> for ProtocolError {
    fn from(error: prost::DecodeError) -> Self {
        Self::Decode(error)
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Error contracts stay beside the error types"
)]
mod tests {
    use super::{PairingError, PairingStep, ProtocolError};
    use core::error::Error as _;

    #[test]
    fn pairing_steps_have_stable_labels() {
        let cases = [
            (PairingStep::SendEncryption, "send_encryption"),
            (PairingStep::ReceiveEncryption, "receive_encryption"),
            (PairingStep::SendResult, "send_result"),
            (PairingStep::ReceiveResult, "receive_result"),
        ];

        for (step, expected) in cases {
            assert_eq!(step.as_str(), expected);
        }
    }

    #[test]
    fn pairing_error_preserves_its_step_and_source() {
        let error = PairingError::new(
            PairingStep::ReceiveEncryption,
            ProtocolError::Disconnected,
        );

        assert_eq!(error.step(), PairingStep::ReceiveEncryption);
        assert!(matches!(error.source_error(), ProtocolError::Disconnected));
        assert_eq!(
            error.to_string(),
            concat!(
                "paired-key receive_encryption failed: ",
                "peer disconnected during the share"
            )
        );
        assert!(
            error
                .source()
                .and_then(
                    <dyn core::error::Error>::downcast_ref::<ProtocolError>
                )
                .is_some()
        );

        assert!(matches!(error.into_source(), ProtocolError::Disconnected));
    }
}
