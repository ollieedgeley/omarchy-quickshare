//! OS-medium adapters for the core generic connection-stream seam.

use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::bus::DbusBytePipe;
use crate::classic::ClassicSocket;
use crate::gatt::WeaveSocket;
use crate::l2cap::L2capChannel;
use crate::radio::{Error, ErrorKind};

use crate::weave::{Assembler, recv_data, send_data};

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(60);

fn adapter_failure(
    medium: &'static str,
    operation: &'static str,
    error: &Error,
) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "medium_io",
        medium,
        operation,
        outcome = "failure",
        io_error_kind = ?error.kind()
    );
}

fn selected_medium(medium: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "medium",
        operation = "select",
        outcome = "selected",
        medium
    );
}

/// A bounded authenticated byte stream independent of BlueZ medium.
#[derive(Debug)]
pub struct BluetoothIo {
    /// Raw Classic/L2CAP pipe or framed GATT weave pipe.
    inner: IoInner,
    /// Bound for one blocking stream read.
    read_timeout: Duration,
}

#[derive(Debug)]
enum IoInner {
    /// Unframed Classic or L2CAP socket.
    Raw {
        /// BlueZ socket.
        pipe: DbusBytePipe,
        /// Unconsumed bytes from the last kernel read.
        leftover: Mutex<Vec<u8>>,
        /// Selected raw Bluetooth medium.
        medium: &'static str,
    },
    /// GATT weave with layer A/B framing before Connections.
    Weave {
        /// Acquired characteristic sockets.
        pipe: DbusBytePipe,
        /// Layer-A reassembly state.
        assembler: Mutex<Assembler>,
        /// Leftover layer-C bytes for `Read`.
        leftover: Mutex<Vec<u8>>,
        /// Whether `CONNECTION_REQUEST` was sent.
        requested: Mutex<bool>,
    },
}

impl BluetoothIo {
    /// Wraps a production Classic socket.
    pub(crate) fn classic(socket: ClassicSocket) -> Result<Self, Error> {
        let pipe = socket.into_dbus_pipe().inspect_err(|error| {
            adapter_failure("bluetooth_classic", "open", error);
        })?;
        selected_medium("bluetooth_classic");
        Ok(Self {
            inner: IoInner::Raw {
                pipe,
                leftover: Mutex::new(Vec::new()),
                medium: "bluetooth_classic",
            },
            read_timeout: DEFAULT_READ_TIMEOUT,
        })
    }

    /// Wraps a production L2CAP channel.
    pub(crate) fn l2cap(channel: L2capChannel) -> Result<Self, Error> {
        let pipe = channel.into_dbus_pipe().inspect_err(|error| {
            adapter_failure("bluetooth_l2cap", "open", error);
        })?;
        selected_medium("bluetooth_l2cap");
        Ok(Self {
            inner: IoInner::Raw {
                pipe,
                leftover: Mutex::new(Vec::new()),
                medium: "bluetooth_l2cap",
            },
            read_timeout: DEFAULT_READ_TIMEOUT,
        })
    }

    /// Wraps a production GATT weave socket with layer A/B framing.
    pub(crate) fn weave(socket: WeaveSocket) -> Result<Self, Error> {
        let pipe = socket
            .into_dbus_pipe()
            .inspect_err(|error| adapter_failure("ble", "open", error))?;
        selected_medium("ble");
        Ok(Self {
            inner: IoInner::Weave {
                pipe,
                assembler: Mutex::new(Assembler::new()),
                leftover: Mutex::new(Vec::new()),
                requested: Mutex::new(false),
            },
            read_timeout: DEFAULT_READ_TIMEOUT,
        })
    }

    /// Shuts down the write half, matching `ConnectionIo::shutdown_write`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the write half cannot be shut down.
    #[inline]
    pub fn shutdown_write(&mut self) -> io::Result<()> {
        let result = match &self.inner {
            IoInner::Raw { pipe, medium, .. } => {
                pipe.shutdown_write().map_err(|error| {
                    adapter_failure(medium, "shutdown_write", &error);
                    io::Error::other(error)
                })
            }
            IoInner::Weave { pipe, .. } => {
                pipe.shutdown_write().map_err(|error| {
                    adapter_failure("ble", "shutdown_write", &error);
                    io::Error::other(error)
                })
            }
        };
        if result.is_ok() {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "medium_io",
                operation = "shutdown_write",
                outcome = "success"
            );
        }
        result
    }
}

impl quickshare_connections::ConnectionIo for BluetoothIo {
    #[inline]
    fn shutdown_write(&mut self) -> io::Result<()> {
        BluetoothIo::shutdown_write(self)
    }

    #[inline]
    fn read_ready(&self) -> io::Result<bool> {
        match &self.inner {
            IoInner::Raw { pipe, leftover, .. }
            | IoInner::Weave { pipe, leftover, .. } => {
                buffered_or_pipe_ready(leftover, pipe)
            }
        }
    }

    #[inline]
    fn set_read_timeout(
        &mut self,
        timeout: Option<Duration>,
    ) -> io::Result<()> {
        self.read_timeout = timeout.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Bluetooth read timeout cannot be disabled",
            )
        })?;
        Ok(())
    }

    #[inline]
    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        Ok(Some(self.read_timeout))
    }

    #[inline]
    fn set_write_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        let result = match &self.inner {
            IoInner::Raw { pipe, medium, .. } => {
                pipe.set_write_timeout(timeout).inspect_err(|error| {
                    adapter_failure(medium, "set_write_timeout", error);
                })
            }
            IoInner::Weave { pipe, .. } => {
                pipe.set_write_timeout(timeout).inspect_err(|error| {
                    adapter_failure("ble", "set_write_timeout", error);
                })
            }
        };
        write_result(result)
    }
}

impl Read for BluetoothIo {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let timeout = self.read_timeout;
        match &self.inner {
            IoInner::Raw {
                pipe,
                leftover,
                medium,
            } => read_buffered(leftover, buf, || {
                let result = pipe.recv(timeout);
                match &result {
                    Ok(bytes) => tracing::trace!(
                        target: "omarchy_quickshare::protocol",
                        stage = "medium_io",
                        medium,
                        operation = "read",
                        byte_count = bytes.len()
                    ),
                    Err(error) if error.kind() == ErrorKind::Closed => {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "medium_io",
                            medium,
                            operation = "read",
                            outcome = "closed",
                            disconnect_origin = "stream_eof"
                        );
                    }
                    Err(error) => adapter_failure(medium, "read", error),
                }
                read_result(result)
            }),
            IoInner::Weave {
                pipe,
                assembler,
                leftover,
                ..
            } => read_weave(pipe, assembler, leftover, buf, timeout),
        }
    }
}

impl Write for BluetoothIo {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &self.inner {
            IoInner::Raw { pipe, medium, .. } => {
                let result = pipe.write(buf);
                if let Err(error) = &result {
                    adapter_failure(medium, "write", error);
                }
                let count = write_result(result)?;
                tracing::trace!(
                    target: "omarchy_quickshare::protocol",
                    stage = "medium_io",
                    medium,
                    operation = "write",
                    byte_count = count
                );
                Ok(count)
            }
            IoInner::Weave {
                pipe, requested, ..
            } => write_weave(pipe, requested, buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        let result = match &self.inner {
            IoInner::Raw { pipe, medium, .. } => {
                pipe.flush().inspect_err(|error| {
                    adapter_failure(medium, "flush", error);
                })
            }
            IoInner::Weave { pipe, .. } => pipe.flush().inspect_err(|error| {
                adapter_failure("ble", "flush", error);
            }),
        };
        write_result(result)
    }
}

fn write_weave(
    pipe: &DbusBytePipe,
    requested: &Mutex<bool>,
    buf: &[u8],
) -> io::Result<usize> {
    let mut requested = requested.lock().map_err(|_| {
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "medium_io",
            medium = "ble",
            operation = "write",
            outcome = "failure",
            reason = "lock_poisoned"
        );
        io::Error::other("weave request lock poisoned")
    })?;
    let request_pending = !*requested;
    let mut sent_pdu = false;
    let mut sent_data = false;
    write_result(send_data(buf, &mut requested, |pdu| {
        let is_data = !request_pending || sent_pdu;
        let result = pipe.send(pdu);
        if result.is_ok() {
            sent_data |= is_data;
            sent_pdu = true;
        }
        match result {
            Err(error) if sent_data && error.kind() == ErrorKind::Timeout => {
                Err(Error::bus(concat!(
                    "weave payload write timed out ",
                    "after a committed PDU"
                )))
            }
            outcome => outcome,
        }
    }))?;
    Ok(buf.len())
}

impl ClassicSocket {
    /// Adapts this socket to the core generic stream seam.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket is not backed by an owned BlueZ fd.
    #[inline]
    pub fn into_io(self) -> Result<BluetoothIo, Error> {
        BluetoothIo::classic(self)
    }
}

impl L2capChannel {
    /// Adapts this channel to the core generic stream seam.
    ///
    /// # Errors
    ///
    /// Returns an error when the channel is not backed by an owned BlueZ fd.
    #[inline]
    pub fn into_io(self) -> Result<BluetoothIo, Error> {
        BluetoothIo::l2cap(self)
    }
}

impl WeaveSocket {
    /// Adapts this weave socket as a framed Connections stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket is not backed by acquired GATT fds.
    #[inline]
    pub fn into_io(self) -> Result<BluetoothIo, Error> {
        BluetoothIo::weave(self)
    }
}

fn buffered_or_pipe_ready(
    leftover: &Mutex<Vec<u8>>,
    pipe: &DbusBytePipe,
) -> io::Result<bool> {
    if !leftover
        .lock()
        .map_err(|_| io::Error::other("stream leftover lock poisoned"))?
        .is_empty()
    {
        return Ok(true);
    }
    pipe.read_ready().map_err(io::Error::other)
}

fn read_buffered(
    leftover: &Mutex<Vec<u8>>,
    buf: &mut [u8],
    fill: impl FnOnce() -> io::Result<Vec<u8>>,
) -> io::Result<usize> {
    let mut leftover = leftover
        .lock()
        .map_err(|_| io::Error::other("stream leftover lock poisoned"))?;
    if leftover.is_empty() {
        *leftover = fill()?;
    }
    let count = leftover.len().min(buf.len());
    buf[..count].copy_from_slice(&leftover[..count]);
    drop(leftover.drain(..count));
    Ok(count)
}

fn read_weave(
    pipe: &DbusBytePipe,
    assembler: &Mutex<Assembler>,
    leftover: &Mutex<Vec<u8>>,
    buf: &mut [u8],
    timeout: Duration,
) -> io::Result<usize> {
    read_buffered(leftover, buf, || {
        let mut assembler = assembler
            .lock()
            .map_err(|_| io::Error::other("weave assembler lock poisoned"))?;
        read_result(recv_data(
            &mut assembler,
            timeout,
            |deadline| {
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or_else(Error::timeout)?;
                pipe.recv(remaining)
            },
            |pdu| pipe.send(pdu),
        ))
    })
}

fn read_result(result: Result<Vec<u8>, Error>) -> io::Result<Vec<u8>> {
    match result {
        Err(error) if error.kind() == ErrorKind::Closed => Ok(Vec::new()),
        outcome => outcome.map_err(io::Error::other),
    }
}

fn write_result<T>(result: Result<T, Error>) -> io::Result<T> {
    match result {
        Err(error) if error.kind() == ErrorKind::Timeout => {
            Err(io::Error::new(io::ErrorKind::TimedOut, error))
        }
        outcome => outcome.map_err(io::Error::other),
    }
}
