//! OS-medium adapters for the core generic connection-stream seam.

use std::io::{self, Read, Write};
use std::sync::Mutex;
use std::time::Duration;

use crate::bus::DbusBytePipe;
use crate::classic::ClassicSocket;
use crate::gatt::WeaveSocket;
use crate::l2cap::L2capChannel;
use crate::radio::Error;
use crate::weave::{Assembler, recv_data, send_data};

/// A bounded authenticated byte stream independent of BlueZ medium.
#[derive(Debug)]
pub struct BluetoothIo {
    /// Raw Classic/L2CAP pipe or framed GATT weave pipe.
    inner: IoInner,
}

#[derive(Debug)]
enum IoInner {
    /// Unframed Classic or L2CAP socket.
    Raw {
        /// BlueZ socket.
        pipe: DbusBytePipe,
        /// Unconsumed bytes from the last kernel read.
        leftover: Mutex<Vec<u8>>,
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
        Ok(Self {
            inner: IoInner::Raw {
                pipe: socket.into_dbus_pipe()?,
                leftover: Mutex::new(Vec::new()),
            },
        })
    }

    /// Wraps a production L2CAP channel.
    pub(crate) fn l2cap(channel: L2capChannel) -> Result<Self, Error> {
        Ok(Self {
            inner: IoInner::Raw {
                pipe: channel.into_dbus_pipe()?,
                leftover: Mutex::new(Vec::new()),
            },
        })
    }

    /// Wraps a production GATT weave socket with layer A/B framing.
    pub(crate) fn weave(socket: WeaveSocket) -> Result<Self, Error> {
        Ok(Self {
            inner: IoInner::Weave {
                pipe: socket.into_dbus_pipe()?,
                assembler: Mutex::new(Assembler::new()),
                leftover: Mutex::new(Vec::new()),
                requested: Mutex::new(false),
            },
        })
    }

    /// Shuts down the write half, matching `ConnectionIo::shutdown_write`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the write half cannot be shut down.
    #[inline]
    pub fn shutdown_write(&mut self) -> io::Result<()> {
        match &self.inner {
            IoInner::Raw { pipe, .. } | IoInner::Weave { pipe, .. } => {
                pipe.shutdown_write().map_err(io::Error::other)
            }
        }
    }
}

impl quickshare_connections::ConnectionIo for BluetoothIo {
    #[inline]
    fn shutdown_write(&mut self) -> io::Result<()> {
        BluetoothIo::shutdown_write(self)
    }
}

impl Read for BluetoothIo {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match &self.inner {
            IoInner::Raw { pipe, leftover } => {
                read_buffered(leftover, buf, || {
                    pipe.recv(Duration::from_secs(60)).map_err(io::Error::other)
                })
            }
            IoInner::Weave {
                pipe,
                assembler,
                leftover,
                ..
            } => read_weave(pipe, assembler, leftover, buf),
        }
    }
}

impl Write for BluetoothIo {
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &self.inner {
            IoInner::Raw { pipe, .. } => {
                pipe.send(buf).map_err(io::Error::other)?;
                Ok(buf.len())
            }
            IoInner::Weave {
                pipe, requested, ..
            } => {
                let mut requested = requested.lock().map_err(|_| {
                    io::Error::other("weave request lock poisoned")
                })?;
                send_data(buf, &mut requested, |pdu| pipe.send(pdu))
                    .map_err(io::Error::other)?;
                Ok(buf.len())
            }
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
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
) -> io::Result<usize> {
    read_buffered(leftover, buf, || {
        let mut assembler = assembler
            .lock()
            .map_err(|_| io::Error::other("weave assembler lock poisoned"))?;
        recv_data(
            &mut assembler,
            Duration::from_secs(60),
            |deadline| pipe.recv(deadline),
            |pdu| pipe.send(pdu),
        )
        .map_err(io::Error::other)
    })
}
