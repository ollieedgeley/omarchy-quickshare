//! Owned BlueZ Unix-fd byte pipe.

use core::time::Duration;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

use crate::radio::Error;

/// Bidirectional stream backed by one or two BlueZ-provided descriptors.
pub(crate) struct DbusBytePipe {
    /// Read half of the BlueZ socket.
    reader: Mutex<UnixStream>,
    /// Write half of the BlueZ socket.
    writer: Mutex<UnixStream>,
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
            writer: Mutex::new(writer),
        })
    }

    /// Wraps GATT acquire write and notify descriptors.
    pub(crate) fn from_pair(reader: UnixStream, writer: UnixStream) -> Self {
        Self {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
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

    /// Writes `bytes` to the BlueZ socket.
    pub(crate) fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        let mut writer = lock(&self.writer)?;
        writer
            .write_all(bytes)
            .map_err(|error| Error::bus(error.to_string()))?;
        writer
            .flush()
            .map_err(|error| Error::bus(error.to_string()))
    }

    /// Shuts down the write half of the BlueZ socket.
    pub(crate) fn shutdown_write(&self) -> Result<(), Error> {
        let writer = lock(&self.writer)?;
        writer
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
