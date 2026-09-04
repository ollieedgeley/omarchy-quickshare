//! Bluetooth Classic discovery, listeners, and sockets.

use alloc::collections::BTreeSet;
use core::time::Duration;
use std::mem;

use crate::radio::{Address, Error, FakeSession, lock};

mod fake;

use self::fake::{accept_fake, next_fake_candidate, recv_fake, send_fake};

/// A discovered Classic peer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassicCandidate {
    /// Peer address.
    address: Address,
}

/// An active Classic inquiry lease.
#[derive(Debug)]
pub struct ClassicDiscovery {
    /// Fake or D-Bus discovery.
    inner: DiscoveryInner,
    /// Addresses already returned.
    seen: BTreeSet<Address>,
}

/// An active Classic listener.
#[derive(Debug)]
pub struct ClassicListener {
    /// Fake or D-Bus listener.
    inner: ListenerInner,
}

/// A connected Classic socket.
#[derive(Debug)]
pub struct ClassicSocket {
    /// Fake or D-Bus socket.
    inner: SocketInner,
}

/// Discovery backend.
#[derive(Debug)]
enum DiscoveryInner {
    /// Production inquiry.
    Dbus(crate::bus::DbusScan),
    /// In-process inquiry.
    Fake {
        /// Owning adapter.
        session: FakeSession,
        /// Lease identifier.
        lease: u64,
        /// Inclusive deadline on the fake clock.
        deadline_ms: u64,
    },
}

/// Listener backend.
#[derive(Debug)]
enum ListenerInner {
    /// Production profile.
    Dbus(crate::bus::DbusClassicListener),
    /// In-process listener.
    Fake {
        /// Owning adapter.
        session: FakeSession,
        /// Registered service UUID.
        uuid: String,
    },
}

/// Socket backend.
#[derive(Debug)]
enum SocketInner {
    /// Production socket.
    Dbus(crate::bus::DbusBytePipe),
    /// In-process byte pipe.
    Fake {
        /// Local adapter.
        session: FakeSession,
        /// Remote adapter.
        peer: Address,
        /// Service UUID.
        uuid: String,
    },
    /// Pipe already moved into a connection stream.
    Taken,
}

impl ClassicCandidate {
    /// Creates a candidate.
    #[must_use]
    #[inline]
    pub(crate) const fn new(address: Address) -> Self {
        Self { address }
    }

    /// Returns the peer address.
    #[must_use]
    #[inline]
    pub const fn address(self) -> Address {
        self.address
    }
}

impl ClassicDiscovery {
    /// Builds a fake discovery lease.
    #[must_use]
    #[inline]
    pub(crate) fn fake(
        session: FakeSession,
        lease: u64,
        deadline_ms: u64,
    ) -> Self {
        Self {
            inner: DiscoveryInner::Fake {
                session,
                lease,
                deadline_ms,
            },
            seen: BTreeSet::new(),
        }
    }

    /// Builds a production discovery lease.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusScan) -> Self {
        Self {
            inner: DiscoveryInner::Dbus(handle),
            seen: BTreeSet::new(),
        }
    }

    /// Returns the next unseen Classic peer.
    ///
    /// # Errors
    ///
    /// Returns a timeout when the deadline elapses without a candidate.
    #[inline]
    pub fn next_candidate(
        &mut self,
    ) -> Result<Option<ClassicCandidate>, Error> {
        match &mut self.inner {
            DiscoveryInner::Dbus(handle) => {
                handle.next_classic_candidate(&mut self.seen)
            }
            DiscoveryInner::Fake {
                session,
                deadline_ms,
                ..
            } => next_fake_candidate(session, *deadline_ms, &mut self.seen),
        }
    }

    /// Stops inquiry.
    ///
    /// # Errors
    ///
    /// Returns an error when BlueZ rejects `StopDiscovery`.
    #[inline]
    pub fn stop(mut self) -> Result<(), Error> {
        self.unregister()
    }

    fn unregister(&mut self) -> Result<(), Error> {
        match &mut self.inner {
            DiscoveryInner::Dbus(handle) => handle.stop(),
            DiscoveryInner::Fake { session, lease, .. } => {
                let mut radio = lock(&session.radio)?;
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _removed = slot.classic_scans.remove(lease);
                }
                Ok(())
            }
        }
    }
}

impl Drop for ClassicDiscovery {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

impl ClassicListener {
    /// Builds a fake listener.
    #[must_use]
    #[inline]
    pub(crate) fn fake(session: FakeSession, uuid: String) -> Self {
        Self {
            inner: ListenerInner::Fake { session, uuid },
        }
    }

    /// Builds a production listener.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusClassicListener) -> Self {
        Self {
            inner: ListenerInner::Dbus(handle),
        }
    }

    /// Accepts one pending Classic socket.
    ///
    /// # Errors
    ///
    /// Returns an error when the listener is closed.
    #[inline]
    pub fn accept(&mut self) -> Result<Option<ClassicSocket>, Error> {
        match &mut self.inner {
            ListenerInner::Dbus(handle) => handle.accept(),
            ListenerInner::Fake { session, uuid } => accept_fake(session, uuid),
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
            ListenerInner::Fake { session, uuid } => {
                let mut radio = lock(&session.radio)?;
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _removed = slot.classic_listeners.remove(uuid);
                    let _inbox = slot.classic_inbox.remove(uuid);
                }
                Ok(())
            }
        }
    }
}

impl Drop for ClassicListener {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

impl ClassicSocket {
    /// Builds a fake socket.
    #[must_use]
    #[inline]
    pub(crate) fn fake(
        session: FakeSession,
        peer: Address,
        uuid: String,
    ) -> Self {
        Self {
            inner: SocketInner::Fake {
                session,
                peer,
                uuid,
            },
        }
    }

    /// Builds a production socket.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusBytePipe) -> Self {
        Self {
            inner: SocketInner::Dbus(handle),
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
            SocketInner::Dbus(handle) => handle.recv(deadline),
            SocketInner::Fake { session, uuid, .. } => {
                recv_fake(session, uuid, deadline)
            }
            SocketInner::Taken => Err(Error::closed()),
        }
    }

    /// Sends one payload.
    ///
    /// # Errors
    ///
    /// Returns an error when the peer socket is gone.
    #[inline]
    pub fn send(&self, bytes: &[u8]) -> Result<(), Error> {
        match &self.inner {
            SocketInner::Dbus(handle) => handle.send(bytes),
            SocketInner::Fake {
                session,
                peer,
                uuid,
            } => send_fake(session, *peer, uuid, bytes),
            SocketInner::Taken => Err(Error::closed()),
        }
    }

    pub(crate) fn into_dbus_pipe(
        mut self,
    ) -> Result<crate::bus::DbusBytePipe, Error> {
        match mem::replace(&mut self.inner, SocketInner::Taken) {
            SocketInner::Dbus(handle) => Ok(handle),
            other => {
                self.inner = other;
                Err(Error::unavailable("fake Classic socket has no owned fd"))
            }
        }
    }
}

impl Drop for ClassicSocket {
    #[inline]
    fn drop(&mut self) {
        if let SocketInner::Fake { session, uuid, .. } = &self.inner {
            if let Ok(mut radio) = lock(&session.radio) {
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _inbox = slot.classic_inbox.remove(uuid);
                }
            }
        }
    }
}
