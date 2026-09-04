//! Queue of BlueZ-provided Unix file descriptors.

use alloc::collections::VecDeque;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use super::DbusBytePipe;
use crate::radio::Error;

/// Thread-safe inbox of connected sockets.
#[derive(Debug)]
pub(crate) struct IncomingSockets {
    /// Pending descriptors.
    fds: Mutex<VecDeque<OwnedFd>>,
    /// Signaled when a descriptor arrives.
    ready: Condvar,
}

impl IncomingSockets {
    /// Creates an empty inbox.
    pub(crate) fn new() -> Self {
        Self {
            fds: Mutex::new(VecDeque::new()),
            ready: Condvar::new(),
        }
    }

    /// Stores one BlueZ-provided descriptor.
    pub(crate) fn push(&self, fd: OwnedFd) -> Result<(), Error> {
        self.fds
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))?
            .push_back(fd);
        self.ready.notify_one();
        Ok(())
    }

    /// Takes one pending descriptor without waiting.
    pub(crate) fn try_take(&self) -> Result<Option<OwnedFd>, Error> {
        Ok(self
            .fds
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))?
            .pop_front())
    }

    /// Waits until a descriptor arrives or `deadline` elapses.
    pub(crate) fn wait(&self, deadline: Instant) -> Result<OwnedFd, Error> {
        let mut guard = self
            .fds
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))?;
        loop {
            if let Some(fd) = guard.pop_front() {
                return Ok(fd);
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::timeout());
            }
            let remaining = deadline.saturating_duration_since(now);
            let (next, timed_out) =
                self.ready
                    .wait_timeout(guard, remaining)
                    .map_err(|error| Error::protocol(error.to_string()))?;
            guard = next;
            if timed_out.timed_out() && guard.is_empty() {
                return Err(Error::timeout());
            }
        }
    }
}

/// Inbox that pairs GATT write and notify halves into one pipe.
#[derive(Debug)]
pub(crate) struct WeaveInbox {
    /// Pending ToPeripheral (write) half.
    write: Mutex<Option<UnixStream>>,
    /// Pending FromPeripheral (notify) half.
    notify: Mutex<Option<UnixStream>>,
    /// Completed bidirectional pipes.
    pipes: Mutex<VecDeque<DbusBytePipe>>,
}

impl WeaveInbox {
    /// Creates an empty inbox.
    pub(crate) fn new() -> Self {
        Self {
            write: Mutex::new(None),
            notify: Mutex::new(None),
            pipes: Mutex::new(VecDeque::new()),
        }
    }

    /// Stores the local write half and completes a pipe when notify exists.
    pub(crate) fn push_write(&self, stream: UnixStream) -> Result<(), Error> {
        *self
            .write
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))? =
            Some(stream);
        self.flush_pair()
    }

    /// Stores the local notify half and completes a pipe when write exists.
    pub(crate) fn push_notify(&self, stream: UnixStream) -> Result<(), Error> {
        *self
            .notify
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))? =
            Some(stream);
        self.flush_pair()
    }

    /// Takes one completed weave pipe without waiting.
    pub(crate) fn try_take(&self) -> Result<Option<DbusBytePipe>, Error> {
        Ok(self
            .pipes
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))?
            .pop_front())
    }
    fn flush_pair(&self) -> Result<(), Error> {
        let mut write = self
            .write
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))?;
        let mut notify = self
            .notify
            .lock()
            .map_err(|error| Error::protocol(error.to_string()))?;
        if write.is_none() || notify.is_none() {
            return Ok(());
        }
        let reader = write.take();
        let writer = notify.take();
        if let (Some(reader), Some(writer)) = (reader, writer) {
            self.pipes
                .lock()
                .map_err(|error| Error::protocol(error.to_string()))?
                .push_back(DbusBytePipe::from_pair(reader, writer));
        }
        Ok(())
    }
}
