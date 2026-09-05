//! Owned BlueZ Unix-fd byte pipe.

use core::time::Duration;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Instant;

use crate::radio::Error;

/// Bidirectional stream backed by one or two BlueZ-provided descriptors.
pub(crate) struct DbusBytePipe {
    /// Read half of the BlueZ socket.
    reader: Mutex<UnixStream>,
    /// Write half and its absolute operation deadline.
    writer: Mutex<PipeWriter>,
}

struct PipeWriter {
    stream: UnixStream,
    deadline: Option<Instant>,
}

impl DbusBytePipe {
    /// Rejects a BlueZ `NewConnection` that did not hand over an fd.
    pub(crate) fn from_new_connection(
        fd: Option<OwnedFd>,
    ) -> Result<Self, Error> {
        let fd =
            fd.ok_or_else(|| Error::protocol("NewConnection missing fd"))?;
        Self::from_owned_fd(fd)
    }

    /// Wraps a bidirectional BlueZ socket.
    pub(crate) fn from_owned_fd(fd: OwnedFd) -> Result<Self, Error> {
        let stream = UnixStream::from(fd);
        let writer = stream
            .try_clone()
            .map_err(|error| Error::bus(error.to_string()))?;
        Ok(Self {
            reader: Mutex::new(stream),
            writer: Mutex::new(PipeWriter {
                stream: writer,
                deadline: None,
            }),
        })
    }

    /// Wraps GATT acquire write and notify descriptors.
    pub(crate) fn from_pair(reader: UnixStream, writer: UnixStream) -> Self {
        Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(PipeWriter {
                stream: writer,
                deadline: None,
            }),
        }
    }

    /// Reads one chunk, waiting at most `deadline`.
    pub(crate) fn recv(&self, deadline: Duration) -> Result<Vec<u8>, Error> {
        let timeout = deadline.max(Duration::from_millis(1));
        let mut reader = lock(&self.reader)?;
        reader
            .set_read_timeout(Some(timeout))
            .map_err(|error| Error::bus(error.to_string()))?;
        let mut buffer = [0_u8; 4096];
        match reader.read(&mut buffer) {
            Ok(0) => Err(Error::closed()),
            Ok(count) => Ok(buffer[..count].to_vec()),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                Err(Error::timeout())
            }
            Err(error) => Err(Error::bus(error.to_string())),
        }
    }

    /// Reports whether a read can make immediate progress.
    pub(crate) fn read_ready(&self) -> Result<bool, Error> {
        use rustix::event::{PollFd, PollFlags, poll};

        let reader = lock(&self.reader)?;
        let mut descriptor = [PollFd::new(&*reader, PollFlags::IN)];
        poll(&mut descriptor, Some(&rustix::event::Timespec::default()))
            .map(|ready| ready != 0)
            .map_err(|error| Error::bus(error.to_string()))
    }

    /// Writes one chunk to the BlueZ socket.
    pub(crate) fn write(&self, bytes: &[u8]) -> Result<usize, Error> {
        let mut writer = lock(&self.writer)?;
        write_once(&mut writer, bytes)
    }

    /// Writes one complete GATT PDU without retrying an ambiguous partial
    /// timeout.
    pub(crate) fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        let mut writer = lock(&self.writer)?;
        let mut written = 0;
        while written < bytes.len() {
            match write_once(&mut writer, &bytes[written..]) {
                Ok(0) => return Err(Error::bus("failed to write whole PDU")),
                Ok(count) => written = written.saturating_add(count),
                Err(error)
                    if written == 0
                        && error.kind() == crate::radio::ErrorKind::Timeout =>
                {
                    return Err(error);
                }
                Err(error) => {
                    return Err(Error::bus(format!(
                        "PDU write failed after {written} bytes: {error}"
                    )));
                }
            }
        }
        writer.stream.flush().map_err(io_error)
    }

    /// Flushes buffered bytes to the BlueZ socket.
    pub(crate) fn flush(&self) -> Result<(), Error> {
        let mut writer = lock(&self.writer)?;
        writer.stream.flush().map_err(io_error)
    }

    /// Bounds each subsequent blocking write or flush.
    pub(crate) fn set_write_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), Error> {
        if timeout.is_zero() {
            return Err(Error::timeout());
        }
        let timeout = timeout.max(Duration::from_micros(1));
        let mut writer = lock(&self.writer)?;
        writer
            .stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| Error::bus(error.to_string()))?;
        writer.deadline = Some(
            Instant::now()
                .checked_add(timeout)
                .ok_or_else(|| Error::protocol("write deadline overflow"))?,
        );
        Ok(())
    }

    /// Shuts down the write half of the BlueZ socket.
    pub(crate) fn shutdown_write(&self) -> Result<(), Error> {
        let writer = lock(&self.writer)?;
        writer
            .stream
            .shutdown(std::net::Shutdown::Write)
            .map_err(|error| Error::bus(error.to_string()))
    }
}

impl core::fmt::Debug for DbusBytePipe {
    fn fmt(
        &self,
        formatter: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result {
        formatter
            .debug_struct("DbusBytePipe")
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, Error> {
    mutex
        .lock()
        .map_err(|error| Error::protocol(error.to_string()))
}

fn write_once(writer: &mut PipeWriter, bytes: &[u8]) -> Result<usize, Error> {
    loop {
        if let Some(deadline) = writer.deadline {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(Error::timeout)?;
            writer
                .stream
                .set_write_timeout(Some(
                    remaining.max(Duration::from_micros(1)),
                ))
                .map_err(|error| Error::bus(error.to_string()))?;
        }
        match writer.stream.write(bytes) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let deadline =
                    writer.deadline.ok_or_else(|| io_error(error))?;
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or_else(Error::timeout)?;
                wait_writable(&writer.stream, remaining)?;
            }
            outcome => return outcome.map_err(io_error),
        }
    }
}

fn wait_writable(stream: &UnixStream, timeout: Duration) -> Result<(), Error> {
    let stream = stream
        .try_clone()
        .map_err(|error| Error::bus(error.to_string()))?;
    let stream = async_io::Async::new_nonblocking(stream)
        .map_err(|error| Error::bus(error.to_string()))?;
    async_io::block_on(futures_lite::future::race(
        async { stream.writable().await.map_err(io_error) },
        async {
            let _ = async_io::Timer::after(timeout).await;
            Err(Error::timeout())
        },
    ))
}

fn io_error(error: std::io::Error) -> Error {
    if is_timeout(error.kind()) {
        Error::timeout()
    } else {
        Error::bus(error.to_string())
    }
}

const fn is_timeout(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn nonblocking_pipe_waits_without_losing_or_duplicating_progress() {
        let (writer, mut reader) = UnixStream::pair().expect("socket pair");
        writer.set_nonblocking(true).expect("nonblocking writer");
        let mut flag_probe = writer.try_clone().expect("clone writer");
        let pipe =
            DbusBytePipe::from_owned_fd(OwnedFd::from(writer)).expect("pipe");
        pipe.set_write_timeout(Duration::from_millis(20))
            .expect("write deadline");
        let payload = vec![0xA5; 8 * 1024 * 1024];
        let mut confirmed = 0;
        let started = Instant::now();

        let timeout = loop {
            match pipe.write(&payload[confirmed..]) {
                Ok(0) => panic!("write made no progress"),
                Ok(count) => confirmed += count,
                Err(error) => break error,
            }
        };
        assert_eq!(timeout.kind(), crate::radio::ErrorKind::Timeout);
        assert!(started.elapsed() < Duration::from_secs(1));

        flag_probe
            .set_write_timeout(None)
            .expect("clear probe timeout");
        let (sent, returned) = mpsc::channel();
        let probe = thread::spawn(move || {
            sent.send(flag_probe.write(&[0]))
                .expect("send probe result");
        });
        let probe_error = returned
            .recv_timeout(Duration::from_millis(100))
            .expect("nonblocking flag preserved")
            .expect_err("full socket would block");
        assert_eq!(probe_error.kind(), std::io::ErrorKind::WouldBlock);
        probe.join().expect("probe thread");

        let drain = thread::spawn(move || {
            let mut received = Vec::new();
            let count = reader.read_to_end(&mut received).expect("drain peer");
            assert_eq!(count, received.len());
            received
        });
        pipe.set_write_timeout(Duration::from_secs(1))
            .expect("resume deadline");
        while confirmed < payload.len() {
            confirmed += pipe
                .write(&payload[confirmed..])
                .expect("resume confirmed suffix");
        }
        pipe.shutdown_write().expect("close writer");
        let received = drain.join().expect("drain thread");
        assert_eq!(received, payload);
    }
}
