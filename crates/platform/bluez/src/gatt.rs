//! GATT application and Nearby weave sockets.

use core::time::Duration;
use std::mem;
use std::sync::Mutex;

use crate::advertisement::powered_slot;
use crate::radio::{
    Address, BleCandidate, Error, FakeSession, lock, next_lease,
};
use crate::token;
use crate::weave::{Assembler, recv_data, send_data};

/// An active GATT weave application.
#[derive(Debug)]
pub struct GattWeaveServer {
    /// Fake or D-Bus registration.
    inner: ServerInner,
}

/// A bidirectional BLE weave socket.
#[derive(Debug)]
pub struct WeaveSocket {
    /// Fake or D-Bus socket.
    inner: SocketInner,
}

/// Server backend.
#[derive(Debug)]
enum ServerInner {
    /// Production GATT application.
    Dbus(crate::bus::DbusGattServer),
    /// In-process GATT application.
    Fake {
        /// Owning adapter.
        session: FakeSession,
        /// Lease identifier.
        lease: u64,
    },
}

#[derive(Debug)]
enum SocketInner {
    /// Production weave path.
    Dbus(Box<DbusWeave>),
    /// In-process byte pipe.
    Fake {
        /// Local adapter.
        session: FakeSession,
        /// Remote adapter.
        peer: Address,
    },
    /// Pipe already moved into a connection stream.
    Taken,
}

/// GATT sockets plus layer-A reassembly.
#[derive(Debug)]
struct DbusWeave {
    /// Acquired characteristic sockets.
    handle: crate::bus::DbusBytePipe,
    /// Layer-A reassembly.
    assembler: Mutex<Assembler>,
    /// Whether `CONNECTION_REQUEST` was sent.
    requested: Mutex<bool>,
}

impl GattWeaveServer {
    /// Builds a fake GATT server.
    #[must_use]
    #[inline]
    pub(crate) fn fake(session: FakeSession, lease: u64) -> Self {
        Self {
            inner: ServerInner::Fake { session, lease },
        }
    }

    /// Builds a production GATT server.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusGattServer) -> Self {
        Self {
            inner: ServerInner::Dbus(handle),
        }
    }

    /// Accepts one pending weave socket.
    ///
    /// # Errors
    ///
    /// Returns an error when the application is closed.
    #[inline]
    pub fn accept(&mut self) -> Result<Option<WeaveSocket>, Error> {
        match &mut self.inner {
            ServerInner::Dbus(handle) => handle.accept(),
            ServerInner::Fake { session, .. } => accept_fake(session),
        }
    }

    /// Unregisters the GATT application.
    ///
    /// # Errors
    ///
    /// Returns an error when BlueZ rejects unregistration.
    #[inline]
    pub fn stop(mut self) -> Result<(), Error> {
        self.unregister()
    }

    fn unregister(&mut self) -> Result<(), Error> {
        match &mut self.inner {
            ServerInner::Dbus(handle) => handle.stop(),
            ServerInner::Fake { session, lease } => {
                let mut radio = lock(&session.radio)?;
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _removed = slot.gatt_servers.remove(lease);
                    slot.weave_inbox.clear();
                }
                Ok(())
            }
        }
    }
}

impl Drop for GattWeaveServer {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

impl WeaveSocket {
    /// Builds a fake weave socket.
    #[must_use]
    #[inline]
    pub(crate) fn fake(session: FakeSession, peer: Address) -> Self {
        Self {
            inner: SocketInner::Fake { session, peer },
        }
    }

    /// Builds a production weave socket.
    pub(crate) fn dbus(handle: crate::bus::DbusBytePipe) -> Self {
        Self {
            inner: SocketInner::Dbus(Box::new(DbusWeave {
                handle,
                assembler: Mutex::new(Assembler::new()),
                requested: Mutex::new(false),
            })),
        }
    }

    /// Receives one queued weave payload.
    ///
    /// # Errors
    ///
    /// Returns a timeout when no payload arrives before `deadline`.
    #[inline]
    pub fn recv(&self, deadline: Duration) -> Result<Vec<u8>, Error> {
        match &self.inner {
            SocketInner::Dbus(state) => {
                let mut assembler = state
                    .assembler
                    .lock()
                    .map_err(|error| Error::protocol(error.to_string()))?;
                recv_data(
                    &mut assembler,
                    deadline,
                    |deadline| state.handle.recv(deadline),
                    |pdu| state.handle.send(pdu),
                )
            }
            SocketInner::Fake { session, .. } => recv_fake(session, deadline),
            SocketInner::Taken => Err(Error::closed()),
        }
    }

    /// Sends one weave payload.
    ///
    /// # Errors
    ///
    /// Returns an error when the peer socket is gone.
    #[inline]
    pub fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        match &self.inner {
            SocketInner::Dbus(state) => {
                let mut requested = state
                    .requested
                    .lock()
                    .map_err(|error| Error::protocol(error.to_string()))?;
                send_data(bytes, &mut requested, |pdu| state.handle.send(pdu))
            }
            SocketInner::Fake { session, peer } => {
                send_fake(session, *peer, bytes)
            }
            SocketInner::Taken => Err(Error::closed()),
        }
    }

    pub(crate) fn into_dbus_pipe(
        mut self,
    ) -> Result<crate::bus::DbusBytePipe, Error> {
        match mem::replace(&mut self.inner, SocketInner::Taken) {
            SocketInner::Dbus(state) => Ok(state.handle),
            other => {
                self.inner = other;
                Err(Error::unavailable("fake weave socket has no owned fd"))
            }
        }
    }
}

impl FakeSession {
    /// Connects to a peer GATT weave server.
    pub(crate) fn connect_gatt_weave(
        &self,
        candidate: &BleCandidate,
        deadline: Duration,
    ) -> Result<WeaveSocket, Error> {
        let mut radio = lock(&self.radio)?;
        if radio.now_ms >= deadline_ms(&radio, deadline) {
            return Err(Error::timeout());
        }
        let peer = radio
            .adapters
            .get_mut(&candidate.address())
            .ok_or_else(|| Error::unavailable("peer adapter is missing"))?;
        if !peer.powered || peer.gatt_servers.is_empty() {
            return Err(Error::unavailable("peer GATT weave is not listening"));
        }
        peer.weave_inbox.push_back(token::encode(self.address));
        Ok(WeaveSocket::fake(self.clone(), candidate.address()))
    }

    /// Registers a fake GATT weave server.
    pub(crate) fn serve_gatt_weave(&self) -> Result<GattWeaveServer, Error> {
        let mut radio = lock(&self.radio)?;
        let lease = next_lease(&mut radio.next_lease);
        let slot = powered_slot(&mut radio.adapters, self.address)?;
        let _inserted = slot.gatt_servers.insert(lease);
        Ok(GattWeaveServer::fake(self.clone(), lease))
    }
}

impl Drop for WeaveSocket {
    #[inline]
    fn drop(&mut self) {
        if let SocketInner::Fake { session, .. } = &self.inner {
            if let Ok(mut radio) = lock(&session.radio) {
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    slot.weave_inbox.clear();
                }
            }
        }
    }
}

/// Accepts a fake inbound weave socket.
fn accept_fake(session: &FakeSession) -> Result<Option<WeaveSocket>, Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&session.address)
        .ok_or_else(|| Error::closed())?;
    let Some(raw) = slot.weave_inbox.pop_front() else {
        return Ok(None);
    };
    let peer = token::parse(&raw)?;
    Ok(Some(WeaveSocket::fake(session.clone(), peer)))
}

/// Pushes bytes into the peer inbox.
fn send_fake(
    session: &FakeSession,
    peer: Address,
    bytes: &[u8],
) -> Result<(), Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&peer)
        .ok_or_else(|| Error::unavailable("peer adapter is missing"))?;
    slot.weave_inbox.push_back(bytes.to_vec());
    Ok(())
}

/// Pops local inbox bytes or times out.
fn recv_fake(
    session: &FakeSession,
    deadline: Duration,
) -> Result<Vec<u8>, Error> {
    let mut radio = lock(&session.radio)?;
    let now_ms = radio.now_ms;
    let limit = now_ms.saturating_add(as_millis(deadline));
    let slot = radio
        .adapters
        .get_mut(&session.address)
        .ok_or_else(|| Error::closed())?;
    if let Some(bytes) = token::pop_payload(&mut slot.weave_inbox) {
        return Ok(bytes);
    }
    if now_ms >= limit {
        Err(Error::timeout())
    } else {
        Err(Error::timeout())
    }
}

fn as_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn deadline_ms(radio: &crate::radio::RadioInner, deadline: Duration) -> u64 {
    radio.now_ms.saturating_add(as_millis(deadline))
}
