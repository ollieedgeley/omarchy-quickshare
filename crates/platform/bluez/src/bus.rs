//! Live `org.bluez` D-Bus session.

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath, Value};

use crate::advertisement::{
    BleAdvertisement, ReceiverAdvertisement, service_uuid,
};
use crate::classic::{
    ClassicCandidate, ClassicDiscovery, ClassicListener, ClassicSocket,
};
use crate::gatt::{GattWeaveServer, WeaveSocket};
use crate::l2cap::{L2capChannel, L2capListener};
use crate::monitor::BleScan;
use crate::radio::{Address, BleCandidate, Error};

mod gatt_app;
mod objects;
mod pipe;
mod profile;
mod scan;
mod sockets;

pub(crate) use pipe::DbusBytePipe;
use sockets::{IncomingSockets, WeaveInbox};

/// A live BlueZ adapter session.
#[derive(Clone)]
pub(crate) struct DbusSession {
    /// Shared D-Bus connection for this adapter.
    pub(super) connection: Connection,
    /// Adapter object path, such as `/org/bluez/hci0`.
    adapter_path: OwnedObjectPath,
    /// Monotonic suffix for exported object paths.
    next_object: Arc<AtomicU64>,
    /// Holders of the single adapter `StartDiscovery` session.
    discovery: Arc<Mutex<DiscoveryHolders>>,
}

/// Refcount for the one valid BlueZ discovery session.
struct DiscoveryHolders {
    /// Live BLE/Classic leases sharing `StartDiscovery`.
    count: u32,
}

/// Active `StartDiscovery` lease.
pub(crate) struct DbusScan {
    /// Session used to stop discovery.
    pub(super) session: DbusSession,
    /// Wall-clock deadline.
    pub(super) deadline: Instant,
    /// Whether `StopDiscovery` already ran.
    pub(super) stopped: bool,
}

/// Active LE advertisement registration.
pub(crate) struct DbusAdvertisement {
    /// Session used to unregister.
    pub(super) session: DbusSession,
    /// Exported advertisement path.
    pub(super) path: OwnedObjectPath,
    /// Whether unregistration already ran.
    pub(super) stopped: bool,
}

/// Active GATT application registration.
pub(crate) struct DbusGattServer {
    /// Session used to unregister.
    pub(super) session: DbusSession,
    /// Application object path.
    pub(super) path: OwnedObjectPath,
    /// Child object paths to remove.
    pub(super) children: Vec<OwnedObjectPath>,
    /// Incoming acquired sockets.
    pub(super) incoming: Arc<WeaveInbox>,
    /// Whether unregistration already ran.
    pub(super) stopped: bool,
}

/// Active L2CAP profile registration.
pub(crate) struct DbusL2capListener {
    /// Session used to unregister.
    pub(super) session: DbusSession,
    /// Profile object path.
    pub(super) path: OwnedObjectPath,
    /// Incoming NewConnection sockets.
    pub(super) incoming: Arc<IncomingSockets>,
    /// Whether unregistration already ran.
    pub(super) stopped: bool,
}

/// Active Classic profile registration.
pub(crate) struct DbusClassicListener {
    /// Session used to unregister.
    pub(super) session: DbusSession,
    /// Profile object path.
    pub(super) path: OwnedObjectPath,
    /// Incoming NewConnection sockets.
    pub(super) incoming: Arc<IncomingSockets>,
    /// Whether unregistration already ran.
    pub(super) stopped: bool,
}

/// Exported LE advertisement object.
struct LeAdvertisement {
    /// Service data for `0xFEF3`.
    service_data: HashMap<String, Vec<u8>>,
}

#[zbus::interface(name = "org.bluez.LEAdvertisement1")]
impl LeAdvertisement {
    /// Releases the advertisement when BlueZ unregisters it.
    fn release(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }

    #[zbus(property, name = "ServiceData")]
    fn service_data(&self) -> HashMap<String, Vec<u8>> {
        self.service_data.clone()
    }

    #[zbus(property, name = "ServiceUUIDs")]
    fn service_uuids(&self) -> Vec<String> {
        vec![String::from(service_uuid())]
    }

    #[zbus(property, name = "Type")]
    fn type_(&self) -> &str {
        "peripheral"
    }
}

impl DbusSession {
    /// Connects to `org.bluez` on a bus address.
    pub(crate) fn connect(address: &str) -> Result<Self, Error> {
        let connection = zbus::blocking::connection::Builder::address(address)
            .map_err(|error| Error::bus(error.to_string()))?
            .build()
            .map_err(|error| Error::bus(error.to_string()))?;
        Self::from_connection(connection)
    }

    /// Connects to the system `org.bluez` service.
    pub(crate) fn system() -> Result<Self, Error> {
        let connection = Connection::system()
            .map_err(|error| Error::bus(error.to_string()))?;
        Self::from_connection(connection)
    }

    /// Registers a connectable Quick Share advertisement.
    pub(crate) fn advertise_receiver(
        &self,
        advertisement: ReceiverAdvertisement,
    ) -> Result<BleAdvertisement, Error> {
        let path = self.next_path("advertisement")?;
        let mut service_data = HashMap::new();
        let _previous = service_data.insert(
            String::from(service_uuid()),
            advertisement.service_data().to_vec(),
        );
        consume(
            self.connection
                .object_server()
                .at(&path, LeAdvertisement { service_data }),
        )?;
        let manager = self.proxy("org.bluez.LEAdvertisingManager1")?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        consume(
            manager
                .call_method("RegisterAdvertisement", &(path.clone(), options)),
        )?;
        Ok(BleAdvertisement::dbus(DbusAdvertisement {
            session: self.clone(),
            path,
            stopped: false,
        }))
    }

    /// Requests a Classic connection through BlueZ.
    pub(crate) fn connect_classic(
        &self,
        candidate: &ClassicCandidate,
        service_uuid: &str,
        deadline: Duration,
    ) -> Result<ClassicSocket, Error> {
        self.connect_device_profile(candidate.address(), service_uuid, deadline)
            .map(ClassicSocket::dbus)
    }

    /// Connects GATT and acquires weave characteristic sockets.
    pub(crate) fn connect_gatt_weave(
        &self,
        candidate: &BleCandidate,
        deadline: Duration,
    ) -> Result<WeaveSocket, Error> {
        self.connect_gatt_sockets(candidate, deadline)
            .map(WeaveSocket::dbus)
    }

    /// Connects an L2CAP profile through BlueZ.
    pub(crate) fn connect_l2cap(
        &self,
        address: Address,
        psm: u16,
        deadline: Duration,
    ) -> Result<L2capChannel, Error> {
        let uuid = format!("0000{psm:04x}-0000-1000-8000-00805f9b34fb");
        self.connect_device_profile(address, &uuid, deadline)
            .map(L2capChannel::dbus)
    }

    /// Starts Classic inquiry.
    pub(crate) fn discover_classic(
        &self,
        deadline: Duration,
    ) -> Result<ClassicDiscovery, Error> {
        self.start_discovery(deadline).map(ClassicDiscovery::dbus)
    }

    /// Registers a Classic profile listener.
    pub(crate) fn listen_classic(
        &self,
        service_uuid: &str,
    ) -> Result<ClassicListener, Error> {
        let (path, incoming) =
            self.register_profile_queue(service_uuid, None, "server")?;
        Ok(ClassicListener::dbus(DbusClassicListener {
            session: self.clone(),
            path,
            incoming,
            stopped: false,
        }))
    }

    /// Registers an L2CAP profile listener.
    pub(crate) fn listen_l2cap(
        &self,
        psm: u16,
    ) -> Result<L2capListener, Error> {
        let uuid = format!("0000{psm:04x}-0000-1000-8000-00805f9b34fb");
        let (path, incoming) =
            self.register_profile_queue(&uuid, Some(psm), "server")?;
        Ok(L2capListener::dbus(DbusL2capListener {
            session: self.clone(),
            path,
            incoming,
            stopped: false,
        }))
    }

    /// Starts LE discovery.
    pub(crate) fn scan_ble(
        &self,
        deadline: Duration,
    ) -> Result<BleScan, Error> {
        self.start_discovery(deadline).map(BleScan::dbus)
    }

    /// Registers a GATT application for weave.
    pub(crate) fn serve_gatt_weave(&self) -> Result<GattWeaveServer, Error> {
        let (path, children, incoming) = self.serve_gatt_tree()?;
        Ok(GattWeaveServer::dbus(DbusGattServer {
            session: self.clone(),
            path,
            children,
            incoming,
            stopped: false,
        }))
    }

    fn adapter_proxy(&self) -> Result<Proxy<'_>, Error> {
        self.proxy("org.bluez.Adapter1")
    }

    fn from_connection(connection: Connection) -> Result<Self, Error> {
        let dbus = DBusProxy::new(&connection)
            .map_err(|error| Error::bus(error.to_string()))?;
        let name = zbus::names::BusName::try_from("org.bluez")
            .map_err(|error| Error::bus(error.to_string()))?;
        let _owner = dbus
            .get_name_owner(name)
            .map_err(|_| Error::unavailable("org.bluez is not running"))?;
        let adapter_path = objects::find_adapter(&connection)?;
        let adapter = Proxy::new(
            &connection,
            "org.bluez",
            adapter_path.clone(),
            "org.bluez.Adapter1",
        )
        .map_err(|error| Error::bus(error.to_string()))?;
        let powered = adapter
            .get_property::<bool>("Powered")
            .map_err(|error| Error::bus(error.to_string()))?;
        if powered {
            Ok(Self {
                connection,
                adapter_path,
                next_object: Arc::new(AtomicU64::new(1)),
                discovery: Arc::new(Mutex::new(DiscoveryHolders { count: 0 })),
            })
        } else {
            Err(Error::unavailable("adapter is not powered"))
        }
    }

    fn next_path(&self, kind: &str) -> Result<OwnedObjectPath, Error> {
        let suffix = self.next_object.fetch_add(1, Ordering::Relaxed);
        ObjectPath::try_from(format!("/org/omarchy/quickshare/{kind}/{suffix}"))
            .map(OwnedObjectPath::from)
            .map_err(|error| Error::protocol(error.to_string()))
    }

    fn proxy(&self, interface: &'static str) -> Result<Proxy<'_>, Error> {
        Proxy::new(
            &self.connection,
            "org.bluez",
            self.adapter_path.clone(),
            interface,
        )
        .map_err(|error| Error::bus(error.to_string()))
    }

    fn start_discovery(&self, deadline: Duration) -> Result<DbusScan, Error> {
        let deadline = Instant::now()
            .checked_add(deadline)
            .ok_or_else(|| Error::protocol("deadline overflow"))?;
        let mut holders = self.lock_discovery()?;
        if holders.count == 0 {
            let adapter = self.adapter_proxy()?;
            let mut filter: HashMap<&str, Value<'_>> = HashMap::new();
            let _transport = filter.insert("Transport", Value::from("auto"));
            let _uuids = filter.insert(
                "UUIDs",
                Value::from(vec![String::from(service_uuid())]),
            );
            consume(adapter.call_method("SetDiscoveryFilter", &(filter,)))?;
            consume(adapter.call_method("StartDiscovery", &()))?;
        }
        holders.count = holders.count.saturating_add(1);
        Ok(DbusScan {
            session: self.clone(),
            deadline,
            stopped: false,
        })
    }

    pub(super) fn stop_discovery(&self) -> Result<(), Error> {
        let mut holders = self.lock_discovery()?;
        let Some(rest) = holders.count.checked_sub(1) else {
            return Ok(());
        };
        holders.count = rest;
        if rest > 0 {
            return Ok(());
        }
        let result =
            consume(self.adapter_proxy()?.call_method("StopDiscovery", &()));
        if result.is_err() {
            holders.count = 1;
        }
        result
    }

    fn lock_discovery(
        &self,
    ) -> Result<MutexGuard<'_, DiscoveryHolders>, Error> {
        self.discovery
            .lock()
            .map_err(|_| Error::protocol("discovery lease lock poisoned"))
    }

    pub(super) fn unregister_advertisement(
        &self,
        path: &OwnedObjectPath,
    ) -> Result<(), Error> {
        let manager = self.proxy("org.bluez.LEAdvertisingManager1")?;
        consume(manager.call_method("UnregisterAdvertisement", &(path,)))?;
        let _removed = self
            .connection
            .object_server()
            .remove::<LeAdvertisement, _>(path)
            .map_err(|error| Error::bus(error.to_string()))?;
        Ok(())
    }

    pub(super) fn unregister_application(
        &self,
        path: &OwnedObjectPath,
        children: &[OwnedObjectPath],
    ) -> Result<(), Error> {
        let manager = self.proxy("org.bluez.GattManager1")?;
        let unregister =
            consume(manager.call_method("UnregisterApplication", &(path,)));
        let unexport =
            gatt_app::unexport_gatt_tree(&self.connection, path, children);
        unregister.and(unexport)
    }

    pub(super) fn unregister_profile(
        &self,
        path: &OwnedObjectPath,
    ) -> Result<(), Error> {
        let manager = Proxy::new(
            &self.connection,
            "org.bluez",
            "/org/bluez",
            "org.bluez.ProfileManager1",
        )
        .map_err(|error| Error::bus(error.to_string()))?;
        consume(manager.call_method("UnregisterProfile", &(path,)))?;
        let _removed = self
            .connection
            .object_server()
            .remove::<profile::BluezProfile, _>(path)
            .map_err(|error| Error::bus(error.to_string()))?;
        Ok(())
    }
}

impl fmt::Debug for DbusSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbusSession")
            .field("adapter_path", &self.adapter_path.as_str())
            .finish_non_exhaustive()
    }
}

/// Discards a successful D-Bus reply.
fn consume<T>(result: zbus::Result<T>) -> Result<(), Error> {
    result
        .map(drop)
        .map_err(|error| Error::bus(error.to_string()))
}
