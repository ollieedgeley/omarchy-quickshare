//! Local endpoint lifecycle and outbound queue ownership.

use std::io::{self, BufReader};
use std::os::unix::net::UnixListener;

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_request, write_response};
use quickshare_control::request::{Envelope as RequestEnvelope, Request};
use quickshare_control::response::Envelope as ResponseEnvelope;

/// The same-user local endpoint state.
#[derive(Debug, Default)]
pub struct Daemon {
    /// Outbound shares accepted from local clients.
    queued: Vec<RequestEnvelope>,
}

impl Daemon {
    /// Creates an empty local endpoint.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self { queued: Vec::new() }
    }

    /// Returns the number of outbound shares owned by the endpoint.
    #[must_use]
    #[inline]
    pub const fn queued_count(&self) -> usize {
        self.queued.len()
    }

    /// Accepts and queues the next local control request.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket or control record is invalid.
    #[inline]
    pub fn serve_next(&mut self, listener: &UnixListener) -> io::Result<()> {
        let (mut stream, _address) = listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = read_request(&mut reader)?;
        if request.version() != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "client uses an unsupported control protocol",
            ));
        }
        if matches!(request.request(), Request::Status) {
            return write_response(&mut stream, &ResponseEnvelope::ready());
        }
        self.queued.push(request);
        write_response(&mut stream, &ResponseEnvelope::queued())
    }
}
