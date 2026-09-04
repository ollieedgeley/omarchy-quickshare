//! Public BlueZ adapter methods.

use core::time::Duration;
use std::sync::Arc;

use crate::advertisement::{BleAdvertisement, ReceiverAdvertisement};
use crate::bus::DbusSession;
use crate::classic::{
    ClassicCandidate, ClassicDiscovery, ClassicListener, ClassicSocket,
};
use crate::gatt::{GattWeaveServer, WeaveSocket};
use crate::l2cap::{L2capChannel, L2capListener};
use crate::monitor::BleScan;
use crate::radio::{Adapter, AdapterInner, Address, BleCandidate, Error};

impl Adapter {
    /// Advertises a connectable Quick Share BLE receiver.
    ///
    /// # Errors
    ///
    /// Returns an error when the adapter is unusable or BlueZ rejects the
    /// advertisement.
    #[inline]
    pub fn advertise_receiver(
        &self,
        advertisement: ReceiverAdvertisement,
    ) -> Result<BleAdvertisement, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => {
                session.advertise_receiver(advertisement)
            }
            AdapterInner::Fake(session) => {
                session.advertise_receiver(advertisement)
            }
        }
    }

    /// Connects a Classic socket to a discovered peer.
    ///
    /// # Errors
    ///
    /// Returns an error when the peer cannot be reached before the deadline.
    #[inline]
    pub fn connect_classic(
        &self,
        candidate: &ClassicCandidate,
        service_uuid: &str,
        deadline: Duration,
    ) -> Result<ClassicSocket, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => {
                session.connect_classic(candidate, service_uuid, deadline)
            }
            AdapterInner::Fake(session) => {
                session.connect_classic(candidate, service_uuid, deadline)
            }
        }
    }

    /// Connects a GATT weave socket to a BLE receiver.
    ///
    /// # Errors
    ///
    /// Returns an error when the GATT application is missing or the deadline
    /// elapses.
    #[inline]
    pub fn connect_gatt_weave(
        &self,
        candidate: &BleCandidate,
        deadline: Duration,
    ) -> Result<WeaveSocket, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => {
                session.connect_gatt_weave(candidate, deadline)
            }
            AdapterInner::Fake(session) => {
                session.connect_gatt_weave(candidate, deadline)
            }
        }
    }

    /// Connects an L2CAP channel to a listening peer.
    ///
    /// # Errors
    ///
    /// Returns an error when no listener exists or the deadline elapses.
    #[inline]
    pub fn connect_l2cap(
        &self,
        address: Address,
        psm: u16,
        deadline: Duration,
    ) -> Result<L2capChannel, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => {
                session.connect_l2cap(address, psm, deadline)
            }
            AdapterInner::Fake(session) => {
                session.connect_l2cap(address, psm, deadline)
            }
        }
    }

    /// Starts Classic inquiry until `deadline`.
    ///
    /// # Errors
    ///
    /// Returns an error when discovery cannot start.
    #[inline]
    pub fn discover_classic(
        &self,
        deadline: Duration,
    ) -> Result<ClassicDiscovery, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => session.discover_classic(deadline),
            AdapterInner::Fake(session) => session.discover_classic(deadline),
        }
    }

    /// Starts a Classic RFCOMM/L2CAP listener for `service_uuid`.
    ///
    /// # Errors
    ///
    /// Returns an error when the profile cannot be registered.
    #[inline]
    pub fn listen_classic(
        &self,
        service_uuid: &str,
    ) -> Result<ClassicListener, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => session.listen_classic(service_uuid),
            AdapterInner::Fake(session) => session.listen_classic(service_uuid),
        }
    }

    /// Starts an L2CAP listener on `psm`.
    ///
    /// # Errors
    ///
    /// Returns an error when the PSM cannot be bound.
    #[inline]
    pub fn listen_l2cap(&self, psm: u16) -> Result<L2capListener, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => session.listen_l2cap(psm),
            AdapterInner::Fake(session) => session.listen_l2cap(psm),
        }
    }

    /// Connects to `org.bluez` on `address`.
    ///
    /// # Errors
    ///
    /// Returns an error when the bus or BlueZ name is unavailable.
    #[inline]
    pub fn on_bus(address: &str) -> Result<Self, Error> {
        DbusSession::connect(address).map(|session| Self {
            inner: Arc::new(AdapterInner::Dbus(session)),
        })
    }

    /// Starts a BLE scan that ends at `deadline`.
    ///
    /// # Errors
    ///
    /// Returns an error when discovery cannot start.
    #[inline]
    pub fn scan_ble(&self, deadline: Duration) -> Result<BleScan, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => session.scan_ble(deadline),
            AdapterInner::Fake(session) => session.scan_ble(deadline),
        }
    }

    /// Registers a GATT weave server for inbound BLE sockets.
    ///
    /// # Errors
    ///
    /// Returns an error when the GATT application cannot be registered.
    #[inline]
    pub fn serve_gatt_weave(&self) -> Result<GattWeaveServer, Error> {
        match &*self.inner {
            AdapterInner::Dbus(session) => session.serve_gatt_weave(),
            AdapterInner::Fake(session) => session.serve_gatt_weave(),
        }
    }

    /// Connects to the system `org.bluez` service.
    ///
    /// # Errors
    ///
    /// Returns an error when the system bus or BlueZ is unavailable. This never
    /// invents a successful adapter.
    #[inline]
    pub fn system() -> Result<Self, Error> {
        DbusSession::system().map(|session| Self {
            inner: Arc::new(AdapterInner::Dbus(session)),
        })
    }
}
