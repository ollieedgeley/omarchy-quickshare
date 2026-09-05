use super::super::{Connection, ConnectionIo, Error, MAX_FRAME_LENGTH};
use prost::Message as _;
use quickshare_wire::connections::OfflineFrame;
use rand_core::{OsRng, RngCore as _};
use std::{
    io::{self, Read, Write},
    time::Instant,
};

pub(super) fn send_plain<Stream>(
    stream: &mut Stream,
    frame: &OfflineFrame,
) -> Result<(), Error>
where
    Stream: Write,
{
    write(stream, &frame.encode_to_vec())
}

pub(super) fn receive_plain<Stream>(
    stream: &mut Stream,
) -> Result<OfflineFrame, Error>
where
    Stream: Read,
{
    let bytes = read(stream)?;
    OfflineFrame::decode(bytes.as_slice()).map_err(|error| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "decode",
            outcome = "rejected",
            reason = "invalid_protobuf",
            "setup frame rejected"
        );
        error.into()
    })
}

pub(super) fn write<Stream>(
    stream: &mut Stream,
    bytes: &[u8],
) -> Result<(), Error>
where
    Stream: Write + ?Sized,
{
    if bytes.len() > MAX_FRAME_LENGTH {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "framing",
            operation = "write",
            outcome = "rejected",
            reason = "frame_too_large",
            byte_count = bytes.len(),
            "connection_io"
        );
        return Err(Error::FrameTooLarge);
    }
    let length = u32::try_from(bytes.len()).map_err(|_| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "framing",
            operation = "write",
            outcome = "rejected",
            reason = "frame_too_large",
            byte_count = bytes.len(),
            "connection_io"
        );
        Error::FrameTooLarge
    })?;
    stream
        .write_all(&length.to_be_bytes())
        .inspect_err(|error| io_rejected("write", "prefix", error))?;
    stream
        .write_all(bytes)
        .inspect_err(|error| io_rejected("write", "body", error))?;
    stream
        .flush()
        .inspect_err(|error| io_rejected("flush", "body", error))?;
    tracing::trace!(
        target: "omarchy_quickshare::protocol",
        stage = "framing",
        operation = "write",
        outcome = "locally_written",
        byte_count = bytes.len(),
        "connection frame written"
    );
    Ok(())
}

struct DeadlineReader<'stream> {
    stream: &'stream mut dyn ConnectionIo,
    deadline: Option<Instant>,
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if let Some(deadline) = self.deadline {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|duration| !duration.is_zero())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::TimedOut, "read deadline")
                })?;
            self.stream.set_read_timeout(remaining)?;
        }
        self.stream.read(bytes)
    }
}

pub(super) fn read<Stream>(stream: &mut Stream) -> Result<Vec<u8>, Error>
where
    Stream: Read + ?Sized,
{
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix[..1]).inspect_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "framing",
                operation = "read",
                outcome = "disconnected",
                reason = "clean_eof",
                disconnect_origin = "stream_eof",
                io_error_kind = "unexpected_eof",
                "connection_io"
            );
        } else {
            io_rejected("read", "prefix", error);
        }
    })?;
    read_frame_remainder(
        stream,
        &mut prefix[1..],
        "prefix",
        "truncated_prefix",
    )?;
    let size = frame_size(prefix)?;
    let mut bytes = vec![0; size];
    read_frame_remainder(stream, &mut bytes, "body", "truncated_body")?;
    tracing::trace!(
        target: "omarchy_quickshare::protocol",
        stage = "framing",
        operation = "read",
        outcome = "completed",
        byte_count = size,
        "connection frame read"
    );
    Ok(bytes)
}

fn frame_size(prefix: [u8; 4]) -> Result<usize, Error> {
    let size = usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "framing",
            operation = "read",
            outcome = "rejected",
            reason = "frame_too_large",
            "connection_io"
        );
        Error::FrameTooLarge
    })?;
    if size > MAX_FRAME_LENGTH {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "framing",
            operation = "read",
            outcome = "rejected",
            reason = "frame_too_large",
            byte_count = size,
            "connection_io"
        );
        return Err(Error::FrameTooLarge);
    }
    Ok(size)
}

fn read_frame_remainder<Stream>(
    stream: &mut Stream,
    bytes: &mut [u8],
    boundary: &'static str,
    truncated_reason: &'static str,
) -> Result<(), Error>
where
    Stream: Read + ?Sized,
{
    stream.read_exact(bytes).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "framing",
                operation = "read",
                outcome = "rejected",
                reason = truncated_reason,
                disconnect_origin = "stream_eof",
                io_error_kind = "unexpected_eof",
                "connection_io"
            );
            io::Error::from(io::ErrorKind::InvalidData).into()
        } else {
            io_rejected("read", boundary, &error);
            error.into()
        }
    })
}

fn io_rejected(
    operation: &'static str,
    boundary: &'static str,
    error: &io::Error,
) {
    let reason = if error.kind() == io::ErrorKind::TimedOut {
        "deadline"
    } else {
        boundary
    };
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "framing",
        operation,
        outcome = "rejected",
        reason,
        io_error_kind = io_error_kind(error.kind()),
        "connection_io"
    );
}

pub(super) fn io_error_kind(kind: io::ErrorKind) -> &'static str {
    if kind == io::ErrorKind::NotFound {
        return "not_found";
    }
    if kind == io::ErrorKind::PermissionDenied {
        return "permission_denied";
    }
    if kind == io::ErrorKind::ConnectionRefused {
        return "connection_refused";
    }
    if kind == io::ErrorKind::ConnectionReset {
        return "connection_reset";
    }
    if kind == io::ErrorKind::ConnectionAborted {
        return "connection_aborted";
    }
    if kind == io::ErrorKind::NotConnected {
        return "not_connected";
    }
    if kind == io::ErrorKind::AddrInUse {
        return "address_in_use";
    }
    if kind == io::ErrorKind::AddrNotAvailable {
        return "address_not_available";
    }
    if kind == io::ErrorKind::BrokenPipe {
        return "broken_pipe";
    }
    other_io_error_kind(kind)
}

/// Maps the remaining common I/O failures to safe static names.
fn other_io_error_kind(kind: io::ErrorKind) -> &'static str {
    if kind == io::ErrorKind::AlreadyExists {
        return "already_exists";
    }
    if kind == io::ErrorKind::WouldBlock {
        return "would_block";
    }
    if kind == io::ErrorKind::InvalidInput {
        return "invalid_input";
    }
    if kind == io::ErrorKind::InvalidData {
        return "invalid_data";
    }
    if kind == io::ErrorKind::TimedOut {
        return "timed_out";
    }
    if kind == io::ErrorKind::WriteZero {
        return "write_zero";
    }
    if kind == io::ErrorKind::Interrupted {
        return "interrupted";
    }
    if kind == io::ErrorKind::UnexpectedEof {
        return "unexpected_eof";
    }
    if kind == io::ErrorKind::OutOfMemory {
        return "out_of_memory";
    }
    "other"
}

/// Generates a fresh initialization vector for one encrypted frame.
pub(super) fn iv() -> [u8; 16] {
    let mut iv = [0; 16];
    OsRng.fill_bytes(&mut iv);
    iv
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Connection methods split handshake, transfer, and upgrade"
)]
impl Connection {
    pub(super) fn send(&mut self, frame: &OfflineFrame) -> Result<(), Error> {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), iv())
            .map_err(|_| Error::Crypto)?;
        write(&mut self.stream, &encrypted)
    }

    pub(super) fn send_on(
        &mut self,
        stream: &mut dyn ConnectionIo,
        frame: &OfflineFrame,
    ) -> Result<(), Error> {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), iv())
            .map_err(|_| Error::Crypto)?;
        write(stream, &encrypted)
    }

    pub(super) fn recv(&mut self) -> Result<OfflineFrame, Error> {
        let mut reader = DeadlineReader {
            stream: &mut *self.stream,
            deadline: self.read_deadline,
        };
        let encrypted = read(&mut reader)?;
        let bytes = self
            .channel
            .decrypt(&encrypted)
            .map_err(|_| Error::Crypto)?;
        OfflineFrame::decode(bytes.as_slice()).map_err(|error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "frame_decode",
                operation = "receive",
                outcome = "rejected",
                reason = "invalid_protobuf",
                "connection frame rejected"
            );
            error.into()
        })
    }

    pub(super) fn recv_on(
        &mut self,
        stream: &mut dyn ConnectionIo,
    ) -> Result<OfflineFrame, Error> {
        let bytes = self
            .channel
            .decrypt(&read(stream)?)
            .map_err(|_| Error::Crypto)?;
        OfflineFrame::decode(bytes.as_slice()).map_err(|error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "frame_decode",
                operation = "receive_upgrade",
                outcome = "rejected",
                reason = "invalid_protobuf",
                "connection frame rejected"
            );
            error.into()
        })
    }
}
