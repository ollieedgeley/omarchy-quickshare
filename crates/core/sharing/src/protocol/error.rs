use quickshare_connections::Error as ConnectionError;
use std::{error, fmt, io};

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
