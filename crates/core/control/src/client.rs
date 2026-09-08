//! Synchronous, one-request connections to the local control endpoint.

use core::time::Duration;
use std::io::{self, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Instant;

use rand_core::RngCore as _;
use rustix::net::{
    self, AddressFamily, SocketAddrUnix, SocketFlags, SocketType,
};

use crate::codec::{read_response, write_request};
use crate::{CONNECTION_TIMEOUT, PROTOCOL_VERSION};
use crate::{request, response};

/// Socket adapter that reapplies the remaining connection budget on each I/O.
struct DeadlineStream {
    /// Final instant at which the exchange may complete.
    deadline: Instant,
    /// Connected local endpoint socket.
    stream: UnixStream,
}

impl DeadlineStream {
    /// Returns the positive time remaining before the connection expires.
    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Defaults use deadline-aware reads; some defaults are unstable"
)]
impl Read for DeadlineStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.set_read_timeout(Some(self.remaining()?))?;
        self.stream.read(buf).map_err(timeout_error)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Defaults use deadline-aware writes; some defaults are unstable"
)]
impl Write for DeadlineStream {
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.set_write_timeout(Some(self.remaining()?))?;
        self.stream.write(buf).map_err(timeout_error)
    }
}

/// Generates a random 128-bit submission identity as lowercase hexadecimal.
///
/// # Errors
///
/// Returns an I/O error if the operating system cannot provide randomness.
#[inline]
pub fn generate_request_id() -> io::Result<String> {
    let mut bytes = [0_u8; 16];
    rand_core::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut id = String::with_capacity(32);
    for byte in bytes {
        for nibble in [byte >> 4_u8, byte & 0x0f] {
            id.push(char::from(match nibble {
                0..=9 => b'0'.wrapping_add(nibble),
                _ => b'a'.wrapping_add(nibble.wrapping_sub(10)),
            }));
        }
    }
    Ok(id)
}

/// Connects to the local endpoint and exchanges one request envelope.
///
/// # Errors
///
/// Returns an I/O error if connecting, encoding, writing, or reading fails,
/// or if the response uses an unsupported control protocol version.
#[inline]
pub fn exchange(
    socket_path: &Path,
    request: &request::Envelope,
) -> io::Result<response::Envelope> {
    let deadline = Instant::now()
        .checked_add(CONNECTION_TIMEOUT)
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
    let stream = connect(socket_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot reach the local endpoint at {}: {error}; start it \
                 with omarchy-quickshare-daemon",
                socket_path.display()
            ),
        )
    })?;
    exchange_before(stream, request, deadline)
}

/// Exchanges one request on an already-connected stream, then closes it.
///
/// The request and response are each one newline-delimited JSON envelope.
/// A decoded result such as `NotFound` remains a response, not an I/O error.
///
/// # Errors
///
/// Returns an I/O error if encoding, writing, or reading fails, or if the
/// response uses an unsupported control protocol version.
#[inline]
pub fn exchange_stream(
    stream: UnixStream,
    request: &request::Envelope,
) -> io::Result<response::Envelope> {
    let deadline = Instant::now()
        .checked_add(CONNECTION_TIMEOUT)
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
    exchange_before(stream, request, deadline)
}

/// Exchanges one envelope within the deadline shared by all socket I/O.
fn exchange_before(
    stream: UnixStream,
    request: &request::Envelope,
    deadline: Instant,
) -> io::Result<response::Envelope> {
    stream.set_nonblocking(false)?;
    let mut bounded_stream = DeadlineStream { deadline, stream };
    write_request(&mut bounded_stream, request)?;
    let response = read_response(&mut BufReader::new(bounded_stream))?;
    if response.version() != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint uses an unsupported control protocol; reinstall the \
             matching omarchy-quickshare package",
        ));
    }
    Ok(response)
}

/// Opens a nonblocking socket so a full listen queue cannot stall the caller.
#[expect(
    clippy::single_call_fn,
    reason = "Keep socket setup errors in the endpoint connection diagnostic"
)]
fn connect(socket_path: &Path) -> io::Result<UnixStream> {
    let socket = net::socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::NONBLOCK | SocketFlags::CLOEXEC,
        None,
    )?;
    net::connect(&socket, &SocketAddrUnix::new(socket_path)?)?;
    Ok(UnixStream::from(socket))
}

/// Normalizes platform socket timeout errors to the control timeout kind.
fn timeout_error(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::WouldBlock {
        io::Error::from(io::ErrorKind::TimedOut)
    } else {
        error
    }
}
