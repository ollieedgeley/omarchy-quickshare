use super::super::{Connection, ConnectionIo, Error, MAX_FRAME_LENGTH};
pub(super) use super::io_error::io_error_kind;
use super::io_error::io_rejected;
use core::time::Duration;
use prost::Message as _;
use quickshare_wire::connections::OfflineFrame;
use rand_core::{OsRng, RngCore as _};
use std::{
    fmt,
    io::{self, Read, Write},
    time::Instant,
};

pub(super) fn send_plain<Stream>(
    stream: &mut Stream,
    frame: &OfflineFrame,
) -> Result<(), Error>
where
    Stream: Write + ?Sized,
{
    write(stream, &frame.encode_to_vec())
}

pub(super) fn receive_plain<Stream>(
    stream: &mut Stream,
) -> Result<OfflineFrame, Error>
where
    Stream: Read + ?Sized,
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

const CONTROL_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
struct Cancelled;

impl fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("connection operation cancelled")
    }
}

impl core::error::Error for Cancelled {}

fn operation_timeout(
    deadline: Option<Instant>,
    is_cancelled: &mut Option<&mut dyn FnMut() -> bool>,
) -> io::Result<Option<Duration>> {
    if is_cancelled.as_mut().is_some_and(|check| check()) {
        return Err(io::Error::other(Cancelled));
    }
    let Some(end_time) = deadline else {
        return Ok(None);
    };
    let remaining = end_time
        .checked_duration_since(Instant::now())
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::TimedOut, "connection deadline")
        })?;
    Ok(Some(if is_cancelled.is_some() {
        remaining.min(CONTROL_POLL_INTERVAL)
    } else {
        remaining
    }))
}

fn poll_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}

fn retry_timeout(error: &io::Error, deadline: Option<Instant>) -> bool {
    poll_timeout(error) && deadline.is_some_and(|value| value > Instant::now())
}

struct DeadlineReader<'stream, 'cancel> {
    stream: &'stream mut dyn ConnectionIo,
    deadline: Option<Instant>,
    is_cancelled: Option<&'cancel mut dyn FnMut() -> bool>,
}

impl Read for DeadlineReader<'_, '_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            if let Some(timeout) =
                operation_timeout(self.deadline, &mut self.is_cancelled)?
            {
                self.stream.set_read_timeout(Some(timeout))?;
            }
            match self.stream.read(bytes) {
                Err(error) if retry_timeout(&error, self.deadline) => {}
                Err(error)
                    if poll_timeout(&error) && self.deadline.is_some() =>
                {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "connection deadline",
                    ));
                }
                result => return result,
            }
        }
    }
}

struct DeadlineWriter<'stream, 'cancel> {
    stream: &'stream mut dyn ConnectionIo,
    deadline: Instant,
    is_cancelled: &'cancel mut dyn FnMut() -> bool,
}

impl Write for DeadlineWriter<'_, '_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        loop {
            let check: &mut dyn FnMut() -> bool = &mut *self.is_cancelled;
            let mut is_cancelled = Some(check);
            let timeout =
                operation_timeout(Some(self.deadline), &mut is_cancelled)?
                    .ok_or_else(|| {
                        io::Error::other("missing write deadline")
                    })?;
            self.stream.set_write_timeout(timeout)?;
            match self.stream.write(bytes) {
                Err(error) if retry_timeout(&error, Some(self.deadline)) => {}
                Err(error) if poll_timeout(&error) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "connection deadline",
                    ));
                }
                result => return result,
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        loop {
            let check: &mut dyn FnMut() -> bool = &mut *self.is_cancelled;
            let mut is_cancelled = Some(check);
            let timeout =
                operation_timeout(Some(self.deadline), &mut is_cancelled)?
                    .ok_or_else(|| {
                        io::Error::other("missing write deadline")
                    })?;
            self.stream.set_write_timeout(timeout)?;
            match self.stream.flush() {
                Err(error) if retry_timeout(&error, Some(self.deadline)) => {}
                Err(error) if poll_timeout(&error) => {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "connection deadline",
                    ));
                }
                result => return result,
            }
        }
    }
}

fn map_cancelled(error: Error) -> Error {
    if matches!(
        &error,
        Error::Io(value)
            if value.get_ref().is_some_and(
                <dyn core::error::Error + Send + Sync + 'static>::is::<
                    Cancelled,
                >,
            )
    ) {
        Error::Cancelled
    } else {
        error
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

    pub(super) fn recv(&mut self) -> Result<OfflineFrame, Error> {
        let mut reader = DeadlineReader {
            stream: &mut *self.stream,
            deadline: self.read_deadline,
            is_cancelled: None,
        };
        let encrypted = read(&mut reader)?;
        self.decode_encrypted(&encrypted)
    }

    pub(super) fn recv_during(
        &mut self,
        deadline: Instant,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<OfflineFrame, Error> {
        let mut reader = DeadlineReader {
            stream: &mut *self.stream,
            deadline: Some(deadline),
            is_cancelled: Some(is_cancelled),
        };
        let encrypted = read(&mut reader).map_err(map_cancelled)?;
        self.decode_encrypted(&encrypted)
    }

    pub(super) fn recv_if_ready(
        &mut self,
        deadline: Instant,
    ) -> Result<Option<OfflineFrame>, Error> {
        if !self.stream.read_ready()? {
            return Ok(None);
        }
        let prior_timeout = self.stream.read_timeout()?;
        let mut never_cancelled = || false;
        let received =
            self.recv_during(deadline, &mut never_cancelled).map(Some);
        let restored = self.stream.set_read_timeout(prior_timeout);
        match (received, restored) {
            (Ok(frame), Ok(())) => Ok(frame),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    pub(super) fn send_during(
        &mut self,
        frame: &OfflineFrame,
        deadline: Instant,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), iv())
            .map_err(|_| Error::Crypto)?;
        let mut writer = DeadlineWriter {
            stream: &mut *self.stream,
            deadline,
            is_cancelled,
        };
        write(&mut writer, &encrypted).map_err(map_cancelled)
    }

    fn decode_encrypted(
        &mut self,
        encrypted: &[u8],
    ) -> Result<OfflineFrame, Error> {
        let bytes =
            self.channel.decrypt(encrypted).map_err(|_| Error::Crypto)?;
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
}
