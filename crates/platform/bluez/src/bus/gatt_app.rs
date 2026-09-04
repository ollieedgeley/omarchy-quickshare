//! GATT application and characteristic Unix-fd acquisition.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::pipe::DbusBytePipe;
use super::sockets::WeaveInbox;
use super::{DbusSession, consume};
use crate::advertisement::service_uuid;
use crate::radio::{BleCandidate, Error};

/// Weave ToPeripheral characteristic UUID.
pub(crate) const WEAVE_WRITE_UUID: &str =
    "00000100-0004-1000-8000-001a11000101";
/// Weave FromPeripheral characteristic UUID.
pub(crate) const WEAVE_NOTIFY_UUID: &str =
    "00000100-0004-1000-8000-001a11000102";

/// Exported GATT application root.
pub(super) struct GattApplication;

/// Exported GATT service.
pub(super) struct GattService;

/// Exported GATT characteristic that can hand over a Unix fd.
pub(super) struct GattCharacteristic {
    /// Characteristic UUID.
    uuid: String,
    /// Parent service path.
    service: OwnedObjectPath,
    /// Flags advertised to BlueZ.
    flags: Vec<String>,
    /// Inbox of acquired sockets.
    incoming: Arc<WeaveInbox>,
}

#[zbus::interface(name = "org.bluez.GattApplication1")]
impl GattApplication {
    /// Releases the application when BlueZ unregisters it.
    fn release(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }
}

#[zbus::interface(name = "org.bluez.GattService1")]
impl GattService {
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> &str {
        service_uuid()
    }

    #[zbus(property, name = "Primary")]
    fn primary(&self) -> bool {
        true
    }

    #[zbus(property, name = "Includes")]
    fn includes(&self) -> Vec<OwnedObjectPath> {
        Vec::new()
    }
}

#[zbus::interface(name = "org.bluez.GattCharacteristic1")]
impl GattCharacteristic {
    #[zbus(property, name = "UUID")]
    fn uuid(&self) -> &str {
        &self.uuid
    }

    #[zbus(property, name = "Service")]
    fn service(&self) -> OwnedObjectPath {
        self.service.clone()
    }

    #[zbus(property, name = "Flags")]
    fn flags(&self) -> Vec<String> {
        self.flags.clone()
    }

    /// Hands a write socket to BlueZ.
    fn acquire_write(
        &self,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(zbus::zvariant::OwnedFd, u16)> {
        pair_write(&self.incoming)
    }

    /// Hands a notify socket to BlueZ.
    fn acquire_notify(
        &self,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<(zbus::zvariant::OwnedFd, u16)> {
        pair_notify(&self.incoming)
    }
}

impl DbusSession {
    /// Registers a weave GATT application that can accept acquired sockets.
    pub(super) fn serve_gatt_tree(
        &self,
    ) -> Result<(OwnedObjectPath, Vec<OwnedObjectPath>, Arc<WeaveInbox>), Error>
    {
        let app = self.next_path("gatt")?;
        let service = child_path(&app, "service")?;
        let write = child_path(&service, "write")?;
        let notify = child_path(&service, "notify")?;
        let incoming = Arc::new(WeaveInbox::new());
        consume(
            self.connection
                .object_server()
                .at(&app, zbus::fdo::ObjectManager),
        )?;
        consume(self.connection.object_server().at(&app, GattApplication))?;
        consume(self.connection.object_server().at(&service, GattService))?;
        consume(self.connection.object_server().at(
            &write,
            GattCharacteristic {
                uuid: String::from(WEAVE_WRITE_UUID),
                service: service.clone(),
                flags: vec![
                    String::from("write"),
                    String::from("write-without-response"),
                    String::from("acquire-write"),
                ],
                incoming: Arc::clone(&incoming),
            },
        ))?;
        consume(self.connection.object_server().at(
            &notify,
            GattCharacteristic {
                uuid: String::from(WEAVE_NOTIFY_UUID),
                service: service.clone(),
                flags: vec![
                    String::from("notify"),
                    String::from("acquire-notify"),
                ],
                incoming: Arc::clone(&incoming),
            },
        ))?;
        let manager = self.proxy("org.bluez.GattManager1")?;
        let options: HashMap<&str, Value<'_>> = HashMap::new();
        consume(
            manager.call_method("RegisterApplication", &(app.clone(), options)),
        )?;
        Ok((app, vec![service, write, notify], incoming))
    }

    /// Connects GATT weave by acquiring characteristic Unix fds.
    pub(super) fn connect_gatt_sockets(
        &self,
        candidate: &BleCandidate,
        deadline: Duration,
    ) -> Result<DbusBytePipe, Error> {
        let deadline_at = Instant::now()
            .checked_add(deadline)
            .ok_or_else(|| Error::protocol("deadline overflow"))?;
        if Instant::now() >= deadline_at {
            return Err(Error::timeout());
        }
        let path = super::objects::device_path(
            &self.adapter_path,
            candidate.address(),
        )?;
        let device = zbus::blocking::Proxy::new(
            &self.connection,
            "org.bluez",
            path,
            "org.bluez.Device1",
        )
        .map_err(|error| Error::bus(error.to_string()))?;
        consume(device.call_method("Connect", &()))?;
        let _write_characteristic = super::scan::wait_until_or_timeout(
            &self.connection,
            deadline_at,
            || characteristic_path(&self.connection, WEAVE_WRITE_UUID),
        )?;
        let write = acquire_fd(
            &self.connection,
            &characteristic_path(&self.connection, WEAVE_WRITE_UUID)?,
            "AcquireWrite",
        )?;
        let notify = acquire_fd(
            &self.connection,
            &characteristic_path(&self.connection, WEAVE_NOTIFY_UUID)?,
            "AcquireNotify",
        )?;
        Ok(DbusBytePipe::from_pair(notify, write))
    }
}

fn acquire_fd(
    connection: &zbus::blocking::Connection,
    path: &OwnedObjectPath,
    method: &str,
) -> Result<UnixStream, Error> {
    let characteristic = zbus::blocking::Proxy::new(
        connection,
        "org.bluez",
        path.clone(),
        "org.bluez.GattCharacteristic1",
    )
    .map_err(|error| Error::bus(error.to_string()))?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let reply = characteristic
        .call_method(method, &(options,))
        .map_err(|error| Error::bus(error.to_string()))?;
    let (fd, _mtu): (zbus::zvariant::OwnedFd, u16) = reply
        .body()
        .deserialize()
        .map_err(|error| Error::protocol(error.to_string()))?;
    Ok(UnixStream::from(OwnedFd::from(fd)))
}

fn characteristic_path(
    connection: &zbus::blocking::Connection,
    uuid: &str,
) -> Result<OwnedObjectPath, Error> {
    for (path, interfaces) in super::objects::managed_objects(connection)? {
        let Some(properties) = interfaces.get("org.bluez.GattCharacteristic1")
        else {
            continue;
        };
        let Some(value) = properties.get("UUID") else {
            continue;
        };
        let found = String::try_from(value.clone())
            .map_err(|error| Error::protocol(error.to_string()))?;
        if found.eq_ignore_ascii_case(uuid) {
            return Ok(path);
        }
    }
    Err(Error::unavailable("weave GATT characteristic is missing"))
}

fn child_path(
    parent: &OwnedObjectPath,
    name: &str,
) -> Result<OwnedObjectPath, Error> {
    ObjectPath::try_from(format!("{}/{name}", parent.as_str()))
        .map(OwnedObjectPath::from)
        .map_err(|error| Error::protocol(error.to_string()))
}

fn pair_write(
    incoming: &WeaveInbox,
) -> zbus::fdo::Result<(zbus::zvariant::OwnedFd, u16)> {
    let (local, remote) = UnixStream::pair()
        .map_err(|error| zbus::fdo::Error::IOError(error.to_string()))?;
    incoming
        .push_write(local)
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    Ok((zbus::zvariant::OwnedFd::from(OwnedFd::from(remote)), 512))
}

fn pair_notify(
    incoming: &WeaveInbox,
) -> zbus::fdo::Result<(zbus::zvariant::OwnedFd, u16)> {
    let (local, remote) = UnixStream::pair()
        .map_err(|error| zbus::fdo::Error::IOError(error.to_string()))?;
    incoming
        .push_notify(local)
        .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
    Ok((zbus::zvariant::OwnedFd::from(OwnedFd::from(remote)), 512))
}

pub(super) fn unexport_gatt_tree(
    connection: &zbus::blocking::Connection,
    path: &OwnedObjectPath,
    children: &[OwnedObjectPath],
) -> Result<(), Error> {
    if let Some(notify) = children.get(2) {
        let _removed = connection
            .object_server()
            .remove::<GattCharacteristic, _>(notify);
    }
    if let Some(write) = children.get(1) {
        let _removed = connection
            .object_server()
            .remove::<GattCharacteristic, _>(write);
    }
    if let Some(service) = children.first() {
        let _removed =
            connection.object_server().remove::<GattService, _>(service);
    }
    let _application = connection
        .object_server()
        .remove::<GattApplication, _>(path);
    let _manager = connection
        .object_server()
        .remove::<zbus::fdo::ObjectManager, _>(path);
    Ok(())
}
