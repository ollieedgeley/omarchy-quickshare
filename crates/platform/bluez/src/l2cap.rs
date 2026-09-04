//! L2CAP listeners and byte channels.

use crate::advertisement::powered_slot;
use crate::radio::{Address, Error, FakeSession, lock};
use crate::token;
use core::time::Duration;
use std::mem;

/// An active L2CAP listener.
#[derive(Debug)]
pub struct L2capListener {
    /// Fake or D-Bus listener.
    inner: ListenerInner,
}

/// A connected L2CAP channel.
#[derive(Debug)]
pub struct L2capChannel {
    /// Fake or D-Bus channel.
    inner: ChannelInner,
}

/// Listener backend.
#[derive(Debug)]
enum ListenerInner {
    /// Production profile registration.
    Dbus(crate::bus::DbusL2capListener),
    /// In-process listener.
    Fake {
        /// Owning adapter.
        session: FakeSession,
        /// Protocol/service multiplexer.
        psm: u16,
    },
}

/// Channel backend.
#[derive(Debug)]
enum ChannelInner {
    /// Production socket.
    Dbus(crate::bus::DbusBytePipe),
    /// In-process byte pipe.
    Fake {
        /// Local adapter.
        session: FakeSession,
        /// Remote adapter.
        peer: Address,
        /// Protocol/service multiplexer.
        psm: u16,
    },
    /// Pipe already moved into a connection stream.
    Taken,
}

impl L2capListener {
    /// Builds a fake listener.
    #[must_use]
    #[inline]
    pub(crate) fn fake(session: FakeSession, psm: u16) -> Self {
        Self {
            inner: ListenerInner::Fake { session, psm },
        }
    }

    /// Builds a production listener.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusL2capListener) -> Self {
        Self {
            inner: ListenerInner::Dbus(handle),
        }
    }

    /// Accepts one pending L2CAP channel.
    ///
    /// # Errors
    ///
    /// Returns an error when the listener is closed.
    #[inline]
    pub fn accept(&mut self) -> Result<Option<L2capChannel>, Error> {
        match &mut self.inner {
            ListenerInner::Dbus(handle) => handle.accept(),
            ListenerInner::Fake { session, psm } => accept_fake(session, *psm),
        }
    }

    /// Unregisters the listener.
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
            ListenerInner::Dbus(handle) => handle.stop(),
            ListenerInner::Fake { session, psm } => {
                let mut radio = lock(&session.radio)?;
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _removed = slot.l2cap_listeners.remove(psm);
                    let _inbox = slot.l2cap_inbox.remove(psm);
                }
                Ok(())
            }
        }
    }
}

impl Drop for L2capListener {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

impl L2capChannel {
    /// Builds a fake channel.
    #[must_use]
    #[inline]
    pub(crate) fn fake(session: FakeSession, peer: Address, psm: u16) -> Self {
        Self {
            inner: ChannelInner::Fake { session, peer, psm },
        }
    }

    /// Builds a production channel.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusBytePipe) -> Self {
        Self {
            inner: ChannelInner::Dbus(handle),
        }
    }

    /// Receives one queued payload.
    ///
    /// # Errors
    ///
    /// Returns a timeout when no payload arrives before `deadline`.
    #[inline]
    pub fn recv(&self, deadline: Duration) -> Result<Vec<u8>, Error> {
        match &self.inner {
            ChannelInner::Dbus(handle) => handle.recv(deadline),
            ChannelInner::Fake { session, psm, .. } => {
                recv_fake(session, *psm, deadline)
            }
            ChannelInner::Taken => Err(Error::closed()),
        }
    }

    /// Sends one payload.
    ///
    /// # Errors
    ///
    /// Returns an error when the peer channel is gone.
    #[inline]
    pub fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        match &self.inner {
            ChannelInner::Dbus(handle) => handle.send(bytes),
            ChannelInner::Fake { session, peer, psm } => {
                send_fake(session, *peer, *psm, bytes)
            }
            ChannelInner::Taken => Err(Error::closed()),
        }
    }

    pub(crate) fn into_dbus_pipe(
        mut self,
    ) -> Result<crate::bus::DbusBytePipe, Error> {
        match mem::replace(&mut self.inner, ChannelInner::Taken) {
            ChannelInner::Dbus(handle) => Ok(handle),
            other => {
                self.inner = other;
                Err(Error::unavailable("fake L2CAP channel has no owned fd"))
            }
        }
    }
}

impl FakeSession {
    /// Connects to a listening L2CAP PSM.
    pub(crate) fn connect_l2cap(
        &self,
        address: Address,
        psm: u16,
        deadline: Duration,
    ) -> Result<L2capChannel, Error> {
        let mut radio = lock(&self.radio)?;
        if deadline_elapsed(&radio, deadline) {
            return Err(Error::timeout());
        }
        let peer = radio
            .adapters
            .get_mut(&address)
            .ok_or_else(|| Error::unavailable("peer adapter is missing"))?;
        if !peer.powered || !peer.l2cap_listeners.contains(&psm) {
            return Err(Error::unavailable("peer L2CAP is not listening"));
        }
        peer.l2cap_inbox
            .entry(psm)
            .or_default()
            .push_back(token::encode(self.address));
        Ok(L2capChannel::fake(self.clone(), address, psm))
    }

    /// Binds a fake L2CAP listener.
    pub(crate) fn listen_l2cap(
        &self,
        psm: u16,
    ) -> Result<L2capListener, Error> {
        let mut radio = lock(&self.radio)?;
        let slot = powered_slot(&mut radio.adapters, self.address)?;
        if !slot.l2cap_listeners.insert(psm) {
            return Err(Error::protocol("L2CAP PSM already bound"));
        }
        Ok(L2capListener::fake(self.clone(), psm))
    }
}

impl Drop for L2capChannel {
    #[inline]
    fn drop(&mut self) {
        if let ChannelInner::Fake { session, psm, .. } = &self.inner {
            if let Ok(mut radio) = lock(&session.radio) {
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _inbox = slot.l2cap_inbox.remove(psm);
                }
            }
        }
    }
}

fn accept_fake(
    session: &FakeSession,
    psm: u16,
) -> Result<Option<L2capChannel>, Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&session.address)
        .ok_or_else(|| Error::closed())?;
    let Some(inbox) = slot.l2cap_inbox.get_mut(&psm) else {
        return Ok(None);
    };
    let Some(raw) = inbox.pop_front() else {
        return Ok(None);
    };
    let peer = token::parse(&raw)?;
    Ok(Some(L2capChannel::fake(session.clone(), peer, psm)))
}

fn send_fake(
    session: &FakeSession,
    peer: Address,
    psm: u16,
    bytes: &[u8],
) -> Result<(), Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&peer)
        .ok_or_else(|| Error::unavailable("peer adapter is missing"))?;
    slot.l2cap_inbox
        .entry(psm)
        .or_default()
        .push_back(bytes.to_vec());
    Ok(())
}

fn recv_fake(
    session: &FakeSession,
    psm: u16,
    deadline: Duration,
) -> Result<Vec<u8>, Error> {
    let mut radio = lock(&session.radio)?;
    let timed_out = deadline_elapsed(&radio, deadline);
    let slot = radio
        .adapters
        .get_mut(&session.address)
        .ok_or_else(|| Error::closed())?;
    if let Some(inbox) = slot.l2cap_inbox.get_mut(&psm) {
        if let Some(bytes) = token::pop_payload(inbox) {
            return Ok(bytes);
        }
    }
    let _ = timed_out;
    Err(Error::timeout())
}

fn deadline_elapsed(
    radio: &crate::radio::RadioInner,
    deadline: Duration,
) -> bool {
    let limit = radio.now_ms.saturating_add(
        u64::try_from(deadline.as_millis()).unwrap_or(u64::MAX),
    );
    radio.now_ms >= limit
}
