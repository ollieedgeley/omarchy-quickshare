//! Bounded nonblocking progress for ordinary local control connections.

use std::io::{self, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Instant;

use quickshare_control::codec::{read_request, write_response};
use quickshare_control::response::{Envelope, ErrorCode};
use quickshare_control::{
    CONNECTION_TIMEOUT, MAX_MESSAGE_BYTES, PROTOCOL_VERSION,
};

use super::Daemon;

const MAX_PENDING_CONNECTIONS: usize = 32;
const BYTES_PER_TICK: usize = 0x0001_0000;

pub(super) fn accept_pending(
    listener: &UnixListener,
    pending: &mut Vec<Connection>,
) -> io::Result<()> {
    // Limit accept work and retained clients so floods cannot starve events.
    for _accepted in 0..MAX_PENDING_CONNECTIONS {
        match listener.accept() {
            Ok((stream, _address)) => {
                if pending.len() < MAX_PENDING_CONNECTIONS
                    && let Ok(connection) = Connection::new(stream)
                {
                    pending.push(connection);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(super) struct Connection {
    stream: UnixStream,
    deadline: Instant,
    input: Vec<u8>,
    read_buffer: Box<[u8]>,
    output: Option<Vec<u8>>,
    written: usize,
}

impl Connection {
    pub(super) fn new(stream: UnixStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            deadline: Instant::now() + CONNECTION_TIMEOUT,
            input: Vec::new(),
            read_buffer: vec![0_u8; BYTES_PER_TICK].into_boxed_slice(),
            output: None,
            written: 0,
        })
    }

    pub(super) fn advance(&mut self, daemon: &mut Daemon) -> io::Result<bool> {
        if Instant::now() >= self.deadline {
            if self.output.is_none() {
                self.reply(&Envelope::error(ErrorCode::RequestTimeout))?;
                // Try a timeout reply once without extending the deadline.
                let _delivered = self.write_ready();
            }
            return Ok(true);
        }
        if self.output.is_none() {
            if let Some(response) = self.read_ready(daemon) {
                self.reply(&response)?;
            } else {
                return Ok(false);
            }
        }
        Ok(self.write_ready())
    }

    fn read_ready(&mut self, daemon: &mut Daemon) -> Option<Envelope> {
        let bytes = &mut self.read_buffer;
        let capacity =
            bytes.len().min(MAX_MESSAGE_BYTES + 1 - self.input.len());
        match self.stream.read(&mut bytes[..capacity]) {
            Ok(0) => Some(Envelope::error(ErrorCode::InvalidRequest)),
            Ok(count) => {
                let newline =
                    bytes[..count].iter().position(|byte| *byte == b'\n');
                let length = newline.map_or(count, |offset| offset + 1);
                if self.input.len() + length - usize::from(newline.is_some())
                    > MAX_MESSAGE_BYTES
                {
                    return Some(Envelope::error(ErrorCode::MessageTooLarge));
                }
                self.input.reserve_exact(length);
                self.input.extend_from_slice(&bytes[..length]);
                newline.map(|_offset| self.dispatch(daemon))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                None
            }
            Err(_error) => Some(Envelope::error(ErrorCode::InvalidRequest)),
        }
    }

    fn dispatch(&self, daemon: &mut Daemon) -> Envelope {
        match read_request(&mut self.input.as_slice()) {
            Ok(request) if request.version() != PROTOCOL_VERSION => {
                Envelope::error(ErrorCode::UnsupportedVersion)
            }
            Ok(request) => {
                daemon.response_for(&request).unwrap_or_else(|_error| {
                    Envelope::error(ErrorCode::InvalidRequest)
                })
            }
            Err(_error) => Envelope::error(ErrorCode::InvalidRequest),
        }
    }

    fn reply(&mut self, response: &Envelope) -> io::Result<()> {
        // Reuse the bounded request allocation for the one reply.
        let mut output = core::mem::take(&mut self.input);
        output.clear();
        if let Err(error) = write_response(&mut output, response) {
            if error.kind() != io::ErrorKind::FileTooLarge {
                return Err(error);
            }
            output.clear();
            write_response(
                &mut output,
                &Envelope::error(ErrorCode::MessageTooLarge),
            )?;
        }
        self.output = Some(output);
        Ok(())
    }

    fn write_ready(&mut self) -> bool {
        let Some(output) = &self.output else {
            return false;
        };
        let end = output.len().min(self.written + BYTES_PER_TICK);
        match self.stream.write(&output[self.written..end]) {
            Ok(0) => true,
            Ok(count) => {
                self.written += count;
                self.written == output.len()
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                false
            }
            Err(_error) => true,
        }
    }
}
