//! Adapter handles, addresses, and the in-process test radio.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use core::fmt;
use std::error as std_error;
use std::sync::{Arc, Mutex};

use crate::advertisement::ReceiverAdvertisement;
use crate::bus::DbusSession;

/// A Bluetooth device address.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Address([u8; 6]);

/// Failure kind for a BlueZ adapter operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The D-Bus or kernel call failed.
    Bus,
    /// A lease or socket was already stopped.
    Closed,
    /// A peer sent an invalid frame or path.
    Protocol,
    /// The deadline elapsed without a result.
    Timeout,
    /// BlueZ, the adapter, or a required profile is missing.
    Unavailable,
}

/// A BlueZ adapter operation failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    /// Machine-readable class.
    kind: ErrorKind,
    /// Human-readable detail.
    message: String,
}

/// A discovered BLE receiver.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BleCandidate {
    /// Advertiser address.
    address: Address,
    /// Service data for `0xFEF3`.
    service_data: Vec<u8>,
}

/// Production or in-process adapter.
#[derive(Clone, Debug)]
pub struct Adapter {
    /// Shared backend.
    pub(crate) inner: Arc<AdapterInner>,
}

/// Backend owned by one adapter handle.
#[derive(Debug)]
pub(crate) enum AdapterInner {
    /// Live `org.bluez` session.
    Dbus(DbusSession),
    /// Deterministic private radio.
    Fake(FakeSession),
}

/// One adapter attached to a [`testing::FakeRadio`].
#[derive(Clone, Debug)]
pub(crate) struct FakeSession {
    /// Adapter identity on the fake radio.
    pub address: Address,
    /// Shared radio state.
    pub radio: Arc<Mutex<RadioInner>>,
}

/// Shared in-process radio.
#[derive(Debug)]
pub(crate) struct RadioInner {
    /// Fake clock in milliseconds.
    pub now_ms: u64,
    /// Next lease identifier.
    pub next_lease: u64,
    /// Adapters keyed by address.
    pub adapters: BTreeMap<Address, AdapterSlot>,
}

/// Per-adapter fake state.
#[derive(Debug)]
pub(crate) struct AdapterSlot {
    /// Whether the adapter is powered.
    pub powered: bool,
    /// Active BLE receiver advertisements.
    pub advertisements: BTreeMap<u64, ReceiverAdvertisement>,
    /// Active BLE scans.
    pub scans: BTreeSet<u64>,
    /// Active Classic discovery leases.
    pub classic_scans: BTreeSet<u64>,
    /// GATT weave servers.
    pub gatt_servers: BTreeSet<u64>,
    /// Pending weave bytes keyed by local adapter.
    pub weave_inbox: VecDeque<Vec<u8>>,
    /// L2CAP listeners keyed by PSM.
    pub l2cap_listeners: BTreeSet<u16>,
    /// Pending L2CAP bytes keyed by PSM.
    pub l2cap_inbox: BTreeMap<u16, VecDeque<Vec<u8>>>,
    /// Classic listeners keyed by service UUID.
    pub classic_listeners: BTreeSet<String>,
    /// Pending Classic bytes keyed by service UUID.
    pub classic_inbox: BTreeMap<String, VecDeque<Vec<u8>>>,
}

/// Test-only in-process BlueZ radio.
pub mod testing {
    use super::{
        Adapter, AdapterInner, AdapterSlot, Address, FakeSession, RadioInner,
    };
    use crate::token;
    use core::time::Duration;
    use std::sync::{Arc, Mutex};

    /// Deterministic private radio for adapter tests.
    #[derive(Clone, Debug)]
    pub struct FakeRadio {
        /// Shared mutable radio.
        inner: Arc<Mutex<RadioInner>>,
    }

    impl FakeRadio {
        /// Creates an empty powered-off-free radio with clock zero.
        #[must_use]
        #[inline]
        pub fn new() -> Self {
            Self {
                inner: Arc::new(Mutex::new(RadioInner {
                    now_ms: 0,
                    next_lease: 1,
                    adapters: alloc::collections::BTreeMap::new(),
                })),
            }
        }

        /// Attaches one powered adapter.
        ///
        /// # Errors
        ///
        /// Returns an error when the address is already registered.
        #[inline]
        pub fn adapter(
            &self,
            address: Address,
        ) -> Result<Adapter, super::Error> {
            let mut radio = super::lock(&self.inner)?;
            if radio.adapters.contains_key(&address) {
                return Err(super::Error::protocol(
                    "adapter address already exists",
                ));
            }
            let _previous =
                radio.adapters.insert(address, AdapterSlot::powered());
            Ok(Adapter {
                inner: Arc::new(AdapterInner::Fake(FakeSession {
                    address,
                    radio: Arc::clone(&self.inner),
                })),
            })
        }

        /// Advances the fake clock.
        #[inline]
        pub fn advance(&self, by: Duration) {
            if let Ok(mut radio) = super::lock(&self.inner) {
                radio.now_ms = radio.now_ms.saturating_add(as_millis(by));
            }
        }
        /// Encodes a weave connect token for `address`.
        #[must_use]
        #[inline]
        pub fn encode_connect_token(address: Address) -> Vec<u8> {
            token::encode(address)
        }

        /// Parses a weave connect token.
        ///
        /// # Errors
        ///
        /// Returns a protocol error when the token is malformed.
        #[inline]
        pub fn parse_connect_token(
            bytes: &[u8],
        ) -> Result<Address, super::Error> {
            token::parse(bytes)
        }

        /// Injects a raw inbound weave token for `listener`.
        ///
        /// # Errors
        ///
        /// Returns an error when the adapter is missing.
        #[inline]
        pub fn inject_connect_token(
            &self,
            listener: Address,
            token: Vec<u8>,
        ) -> Result<(), super::Error> {
            let mut radio = super::lock(&self.inner)?;
            let slot = radio.adapters.get_mut(&listener).ok_or_else(|| {
                super::Error::unavailable("peer adapter is missing")
            })?;
            slot.weave_inbox.push_back(token);
            Ok(())
        }

        /// Rejects a BlueZ `NewConnection` that did not hand over an fd.
        ///
        /// # Errors
        ///
        /// Returns a protocol error when `fd` is missing.
        #[inline]
        pub fn pipe_from_new_connection(
            fd: Option<std::os::fd::OwnedFd>,
        ) -> Result<(), super::Error> {
            crate::bus::DbusBytePipe::from_new_connection(fd).map(drop)
        }

        /// Two connected Classic streams for prefix-and-body read tests.
        ///
        /// # Errors
        ///
        /// Returns an error when the socket pair cannot be created.
        #[inline]
        pub fn connected_classic_io()
        -> Result<(crate::BluetoothIo, crate::BluetoothIo), super::Error>
        {
            use std::os::fd::OwnedFd;
            use std::os::unix::net::UnixStream;

            let (left, right) = UnixStream::pair()
                .map_err(|error| super::Error::bus(error.to_string()))?;
            let left = crate::ClassicSocket::dbus(
                crate::bus::DbusBytePipe::from_owned_fd(OwnedFd::from(left))?,
            );
            let right = crate::ClassicSocket::dbus(
                crate::bus::DbusBytePipe::from_owned_fd(OwnedFd::from(right))?,
            );
            Ok((left.into_io()?, right.into_io()?))
        }

        /// Removes power from one adapter.
        #[inline]
        pub fn unpower(&self, address: Address) {
            if let Ok(mut radio) = super::lock(&self.inner) {
                if let Some(slot) = radio.adapters.get_mut(&address) {
                    slot.powered = false;
                    slot.advertisements.clear();
                    slot.scans.clear();
                    slot.classic_scans.clear();
                    slot.gatt_servers.clear();
                    slot.l2cap_listeners.clear();
                    slot.classic_listeners.clear();
                }
            }
        }
    }

    impl Default for FakeRadio {
        #[inline]
        fn default() -> Self {
            Self::new()
        }
    }

    /// Converts a duration to whole milliseconds.
    fn as_millis(duration: Duration) -> u64 {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    }
}

impl Address {
    /// Creates an address from six bytes.
    #[must_use]
    #[inline]
    pub const fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// Returns the raw address bytes.
    #[must_use]
    #[inline]
    pub const fn bytes(self) -> [u8; 6] {
        self.0
    }
}

impl BleCandidate {
    /// Creates a candidate from a scan result.
    #[must_use]
    #[inline]
    pub(crate) const fn new(address: Address, service_data: Vec<u8>) -> Self {
        Self {
            address,
            service_data,
        }
    }

    /// Returns the advertiser address.
    #[must_use]
    #[inline]
    pub const fn address(&self) -> Address {
        self.address
    }

    /// Returns the Quick Share service data.
    #[must_use]
    #[inline]
    pub fn service_data(&self) -> &[u8] {
        &self.service_data
    }
}

impl Error {
    /// Creates a D-Bus failure.
    #[must_use]
    #[inline]
    pub(crate) fn bus(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Bus,
            message: message.into(),
        }
    }

    /// Creates a closed-lease failure.
    #[must_use]
    #[inline]
    pub(crate) fn closed() -> Self {
        Self {
            kind: ErrorKind::Closed,
            message: String::from("bluetooth lease is closed"),
        }
    }

    /// Returns the failure class.
    #[must_use]
    #[inline]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Creates a protocol failure.
    #[must_use]
    #[inline]
    pub(crate) fn protocol(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Protocol,
            message: message.into(),
        }
    }

    /// Creates a deadline failure.
    #[must_use]
    #[inline]
    pub(crate) fn timeout() -> Self {
        Self {
            kind: ErrorKind::Timeout,
            message: String::from("bluetooth operation timed out"),
        }
    }

    /// Creates an unavailable-adapter failure.
    #[must_use]
    #[inline]
    pub(crate) fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Unavailable,
            message: message.into(),
        }
    }
}

impl AdapterSlot {
    /// Creates a powered adapter with no leases.
    fn powered() -> Self {
        Self {
            powered: true,
            advertisements: BTreeMap::new(),
            scans: BTreeSet::new(),
            classic_scans: BTreeSet::new(),
            gatt_servers: BTreeSet::new(),
            weave_inbox: VecDeque::new(),
            l2cap_listeners: BTreeSet::new(),
            l2cap_inbox: BTreeMap::new(),
            classic_listeners: BTreeSet::new(),
            classic_inbox: BTreeMap::new(),
        }
    }
}

impl fmt::Display for Address {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", kind_name(self.kind), self.message)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "The standard Error defaults provide the required behavior"
)]
impl std_error::Error for Error {}

/// Locks radio state and maps poison to a protocol error.
pub(crate) fn lock(
    radio: &Mutex<RadioInner>,
) -> Result<std::sync::MutexGuard<'_, RadioInner>, Error> {
    radio
        .lock()
        .map_err(|error| Error::protocol(error.to_string()))
}

/// Allocates the next lease identifier.
pub(crate) fn next_lease(next: &mut u64) -> u64 {
    let lease = *next;
    *next = next.saturating_add(1);
    lease
}

/// Returns a static name for an error kind.
const fn kind_name(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Bus => "bus",
        ErrorKind::Closed => "closed",
        ErrorKind::Protocol => "protocol",
        ErrorKind::Timeout => "timeout",
        ErrorKind::Unavailable => "unavailable",
    }
}
