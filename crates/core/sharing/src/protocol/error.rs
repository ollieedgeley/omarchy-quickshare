use quickshare_connections::Error as ConnectionError;
use std::{error, fmt};

/// Failures while interpreting Sharing messages or transfer events.
#[derive(Debug)]
pub enum ProtocolError {
    /// The encrypted Connections session failed.
    Connection(ConnectionError),
    /// A Sharing protobuf could not be decoded.
    Decode(prost::DecodeError),
    /// The endpoint-info bytes violate the supported layout.
    InvalidAdvertisement,
    /// The Nearby LAN instance bytes violate the supported layout.
    InvalidMdnsInstance,
    /// A Sharing frame has the wrong kind or required field.
    InvalidFrame,
    /// The introduction is not one safe regular file.
    InvalidOffer(&'static str),
    /// The peer rejected the file introduction.
    Rejected,
    /// The declared payload does not match its introduction.
    InvalidPayload,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connection(error) => {
                write!(formatter, "Connections failed: {error}")
            }
            Self::Decode(error) => {
                write!(formatter, "Sharing protobuf failed: {error}")
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

impl From<prost::DecodeError> for ProtocolError {
    fn from(error: prost::DecodeError) -> Self {
        Self::Decode(error)
    }
}
