use quickshare_crypto::SecureChannel;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::{
    collections::{HashMap, VecDeque},
    fmt, io,
    io::{Read, Write},
    net::{Shutdown, TcpStream},
};

mod frame;
mod protocol;
mod upgrade;

pub use upgrade::{
    Medium, UpgradeCredentials, UpgradeDecision, UpgradeEvent, UpgradeState,
};

/// A bounded byte stream that can carry an authenticated Nearby Connections
/// session.
///
/// Platform adapters implement this trait. Core does not depend on an OS
/// medium.
pub trait ConnectionIo: Read + Write + Send {
    /// Closes the local write half after a disconnection frame.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the write half cannot be shut down.
    fn shutdown_write(&mut self) -> io::Result<()>;
}

impl ConnectionIo for TcpStream {
    fn shutdown_write(&mut self) -> io::Result<()> {
        Self::shutdown(self, Shutdown::Write)
    }
}

#[cfg(unix)]
impl ConnectionIo for UnixStream {
    fn shutdown_write(&mut self) -> io::Result<()> {
        Self::shutdown(self, Shutdown::Write)
    }
}

impl ConnectionIo for Box<dyn ConnectionIo> {
    fn shutdown_write(&mut self) -> io::Result<()> {
        (**self).shutdown_write()
    }
}

const MAX_FRAME_LENGTH: usize = 1024 * 1024;
const MAX_PAYLOAD_LENGTH: i64 = 1024 * 1024 * 1024;

/// The identifying data sent in the plaintext connection request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionOptions {
    pub(super) id: String,
    pub(super) info: Vec<u8>,
    pub(super) medium: Medium,
    pub(super) name: String,
}
impl ConnectionOptions {
    /// Creates identifying data for a connection request.
    #[must_use]
    pub fn new<EndpointId, EndpointName>(
        endpoint_id: EndpointId,
        endpoint_name: EndpointName,
    ) -> Self
    where
        EndpointId: Into<String>,
        EndpointName: Into<String>,
    {
        Self {
            id: endpoint_id.into(),
            info: Vec::new(),
            medium: Medium::WifiLan,
            name: endpoint_name.into(),
        }
    }

    /// Adds opaque application identity bytes to the connection request.
    #[must_use]
    pub fn with_endpoint_info(mut self, endpoint_info: Vec<u8>) -> Self {
        self.info = endpoint_info;
        self
    }

    /// Sets the medium that currently carries this connection.
    #[must_use]
    pub const fn with_medium(mut self, medium: Medium) -> Self {
        self.medium = medium;
        self
    }
}

/// Observable input received from a peer.
#[derive(Clone, Debug, Eq, PartialEq)]
#[expect(
    missing_docs,
    reason = "Variant documentation states the field meaning."
)]
#[non_exhaustive]
pub enum Event {
    /// A complete BYTES payload.
    Bytes { id: i64, bytes: Vec<u8> },
    /// The declaration for a FILE payload.
    FileHeader {
        id: i64,
        total_size: i64,
        name: Option<String>,
    },
    /// A FILE payload chunk.
    FileChunk {
        id: i64,
        offset: i64,
        bytes: Vec<u8>,
        is_last: bool,
    },
    /// A keepalive request or acknowledgement.
    KeepAlive { ack: bool, sequence: u32 },
    /// A bandwidth-upgrade negotiation frame.
    Upgrade { event: UpgradeEvent },
    /// The peer requested a clean disconnection or closed the stream.
    Disconnected,
}

/// A connection-layer failure.
#[derive(Debug)]
#[expect(missing_docs, reason = "The error type documents each failure class.")]
#[non_exhaustive]
pub enum Error {
    Io(io::Error),
    Wire(prost::DecodeError),
    FrameTooLarge,
    UnexpectedFrame,
    Rejected,
    Handshake,
    Crypto,
    InvalidPayload,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "stream I/O failed: {error}"),
            Self::Wire(error) => write!(f, "invalid Nearby frame: {error}"),
            Self::FrameTooLarge => {
                f.write_str("Nearby frame exceeds the stream bound")
            }
            Self::UnexpectedFrame => f.write_str("unexpected Nearby frame"),
            Self::Rejected => f.write_str("peer rejected the connection"),
            Self::Handshake => f.write_str("UKEY2 handshake failed"),
            Self::Crypto => {
                f.write_str("encrypted Nearby frame failed verification")
            }
            Self::InvalidPayload => f.write_str("invalid Nearby payload frame"),
        }
    }
}
impl core::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<prost::DecodeError> for Error {
    fn from(error: prost::DecodeError) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PayloadKind {
    Bytes,
    File,
}

#[derive(Clone, Debug)]
struct OutgoingFile {
    id: i64,
    header: quickshare_wire::connections::payload_transfer_frame::PayloadHeader,
}

#[derive(Clone, Debug)]
struct IncomingBytes {
    bytes: Vec<u8>,
    next_offset: i64,
    size: i64,
}

/// An encrypted Nearby Connections relationship over a byte stream.
pub struct Connection {
    stream: Box<dyn ConnectionIo>,
    channel: SecureChannel,
    incoming_bytes: HashMap<i64, IncomingBytes>,
    payloads: HashMap<i64, PayloadKind>,
    incoming_file: Option<i64>,
    outgoing_file: Option<OutgoingFile>,
    pending_events: VecDeque<Event>,
    medium: Medium,
    upgrade: UpgradeState,
    upgrade_host: bool,
    endpoint_id: String,
    /// Four decimal digits derived from the UKEY2 authentication token.
    verification_code: String,
}

impl fmt::Debug for Connection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Connection")
            .field("payloads", &self.payloads)
            .finish_non_exhaustive()
    }
}
