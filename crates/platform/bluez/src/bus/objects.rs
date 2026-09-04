//! BlueZ object-manager discovery and lease cleanup.

use alloc::collections::{BTreeMap, BTreeSet};
use core::fmt;
use std::collections::HashMap;
use std::time::Instant;

use zbus::blocking::Connection;
use zbus::blocking::fdo::ObjectManagerProxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use super::{
    DbusAdvertisement, DbusClassicListener, DbusGattServer, DbusL2capListener,
    DbusScan,
};
use crate::advertisement::service_uuid;
use crate::classic::ClassicCandidate;
use crate::classic::ClassicSocket;
use crate::gatt::WeaveSocket;
use crate::l2cap::L2capChannel;
use crate::radio::{Address, BleCandidate, Error};

impl DbusScan {
    pub(crate) fn next_candidate(
        &self,
        seen: &mut BTreeSet<Address>,
    ) -> Result<Option<BleCandidate>, Error> {
        match collect_ble_candidates(&self.session.connection, seen)? {
            Some(candidate) => Ok(Some(candidate)),
            None => self.none_or_timeout(),
        }
    }

    pub(crate) fn next_classic_candidate(
        &self,
        seen: &mut BTreeSet<Address>,
    ) -> Result<Option<ClassicCandidate>, Error> {
        match collect_classic_candidates(&self.session.connection, seen)? {
            Some(candidate) => Ok(Some(candidate)),
            None => self.none_or_timeout(),
        }
    }

    fn none_or_timeout<T>(&self) -> Result<Option<T>, Error> {
        if Instant::now() >= self.deadline {
            Err(Error::timeout())
        } else {
            Ok(None)
        }
    }

    pub(crate) fn stop(&mut self) -> Result<(), Error> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.session.stop_discovery()
    }
}

impl DbusAdvertisement {
    pub(crate) fn stop(&mut self) -> Result<(), Error> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.session.unregister_advertisement(&self.path)
    }
}

impl DbusGattServer {
    pub(crate) fn accept(&mut self) -> Result<Option<WeaveSocket>, Error> {
        match self.incoming.try_take()? {
            Some(pipe) => Ok(Some(WeaveSocket::dbus(pipe))),
            None => Ok(None),
        }
    }

    pub(crate) fn stop(&mut self) -> Result<(), Error> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.session
            .unregister_application(&self.path, &self.children)
    }
}

impl DbusL2capListener {
    pub(crate) fn accept(&mut self) -> Result<Option<L2capChannel>, Error> {
        accept_profile(&self.incoming, L2capChannel::dbus)
    }

    pub(crate) fn stop(&mut self) -> Result<(), Error> {
        stop_profile(&mut self.stopped, &self.session, &self.path)
    }
}

impl DbusClassicListener {
    pub(crate) fn accept(&mut self) -> Result<Option<ClassicSocket>, Error> {
        accept_profile(&self.incoming, ClassicSocket::dbus)
    }

    pub(crate) fn stop(&mut self) -> Result<(), Error> {
        stop_profile(&mut self.stopped, &self.session, &self.path)
    }
}

fn accept_profile<Channel>(
    incoming: &super::IncomingSockets,
    wrap: impl FnOnce(super::DbusBytePipe) -> Channel,
) -> Result<Option<Channel>, Error> {
    let Some(fd) = incoming.try_take()? else {
        return Ok(None);
    };
    let pipe = super::DbusBytePipe::from_owned_fd(fd)?;
    Ok(Some(wrap(pipe)))
}

fn stop_profile(
    stopped: &mut bool,
    session: &super::DbusSession,
    path: &OwnedObjectPath,
) -> Result<(), Error> {
    if *stopped {
        return Ok(());
    }
    *stopped = true;
    session.unregister_profile(path)
}

impl fmt::Debug for DbusScan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbusScan")
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for DbusAdvertisement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbusAdvertisement")
            .field("path", &self.path.as_str())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for DbusGattServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbusGattServer")
            .field("path", &self.path.as_str())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for DbusL2capListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbusL2capListener")
            .field("path", &self.path.as_str())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for DbusClassicListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DbusClassicListener")
            .field("path", &self.path.as_str())
            .finish_non_exhaustive()
    }
}

pub(super) fn device_path(
    adapter: &OwnedObjectPath,
    address: Address,
) -> Result<OwnedObjectPath, Error> {
    let bytes = address.bytes();
    let suffix = format!(
        "dev_{:02X}_{:02X}_{:02X}_{:02X}_{:02X}_{:02X}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]
    );
    zbus::zvariant::ObjectPath::try_from(format!(
        "{}/{suffix}",
        adapter.as_str()
    ))
    .map(OwnedObjectPath::from)
    .map_err(|error| Error::protocol(error.to_string()))
}

pub(super) fn find_adapter(
    connection: &Connection,
) -> Result<OwnedObjectPath, Error> {
    for (path, interfaces) in managed_objects(connection)? {
        if interfaces.contains_key("org.bluez.Adapter1") {
            return Ok(path);
        }
    }
    Err(Error::unavailable("no BlueZ adapter is present"))
}

fn collect_ble_candidates(
    connection: &Connection,
    seen: &mut BTreeSet<Address>,
) -> Result<Option<BleCandidate>, Error> {
    collect_candidate(connection, seen, |address, device| {
        Ok(service_data_value(device)?
            .map(|service_data| BleCandidate::new(address, service_data)))
    })
}

fn collect_classic_candidates(
    connection: &Connection,
    seen: &mut BTreeSet<Address>,
) -> Result<Option<ClassicCandidate>, Error> {
    collect_candidate(connection, seen, |address, device| {
        Ok(quick_share_device(device)?
            .then_some(ClassicCandidate::new(address)))
    })
}

fn collect_candidate<Candidate>(
    connection: &Connection,
    seen: &mut BTreeSet<Address>,
    mut select: impl FnMut(
        Address,
        &BTreeMap<String, OwnedValue>,
    ) -> Result<Option<Candidate>, Error>,
) -> Result<Option<Candidate>, Error> {
    for (_path, interfaces) in managed_objects(connection)? {
        let Some(device) = interfaces.get("org.bluez.Device1") else {
            continue;
        };
        let Some(address) = device_address(device)? else {
            continue;
        };
        if seen.contains(&address) {
            continue;
        }
        let Some(candidate) = select(address, device)? else {
            continue;
        };
        let _inserted = seen.insert(address);
        return Ok(Some(candidate));
    }
    Ok(None)
}

fn device_address(
    properties: &BTreeMap<String, OwnedValue>,
) -> Result<Option<Address>, Error> {
    let Some(value) = properties.get("Address") else {
        return Ok(None);
    };
    let address = String::try_from(value.clone())
        .map_err(|error| Error::protocol(error.to_string()))?;
    parse_address(&address).map(Some)
}

pub(super) fn managed_objects(
    connection: &Connection,
) -> Result<
    HashMap<OwnedObjectPath, HashMap<String, BTreeMap<String, OwnedValue>>>,
    Error,
> {
    let manager = ObjectManagerProxy::builder(connection)
        .destination("org.bluez")
        .map_err(|error| Error::bus(error.to_string()))?
        .path("/")
        .map_err(|error| Error::bus(error.to_string()))?
        .build()
        .map_err(|error| Error::bus(error.to_string()))?;
    let objects = manager
        .get_managed_objects()
        .map_err(|error| Error::bus(error.to_string()))?;
    let mut converted = HashMap::new();
    for (path, interfaces) in objects {
        let mut named = HashMap::new();
        for (interface, properties) in interfaces {
            let mut values = BTreeMap::new();
            for (key, value) in properties {
                let _previous = values.insert(key, value);
            }
            let _previous = named.insert(interface.to_string(), values);
        }
        let _previous = converted.insert(path, named);
    }
    Ok(converted)
}

fn parse_address(value: &str) -> Result<Address, Error> {
    let mut bytes = [0_u8; 6];
    let mut parts = value.split(':');
    for byte in &mut bytes {
        let part = parts
            .next()
            .ok_or_else(|| Error::protocol("invalid Bluetooth address"))?;
        *byte = u8::from_str_radix(part, 16)
            .map_err(|error| Error::protocol(error.to_string()))?;
    }
    if parts.next().is_some() {
        return Err(Error::protocol("invalid Bluetooth address"));
    }
    Ok(Address::from_bytes(bytes))
}

fn service_data_value(
    properties: &BTreeMap<String, OwnedValue>,
) -> Result<Option<Vec<u8>>, Error> {
    let Some(value) = properties.get("ServiceData") else {
        return Ok(None);
    };
    let map = HashMap::<String, Vec<u8>>::try_from(value.clone())
        .map_err(|error| Error::protocol(error.to_string()))?;
    Ok(map.get(service_uuid()).cloned())
}

fn quick_share_device(
    properties: &BTreeMap<String, OwnedValue>,
) -> Result<bool, Error> {
    let Some(value) = properties.get("UUIDs") else {
        return Ok(false);
    };
    let uuids = Vec::<String>::try_from(value.clone())
        .map_err(|error| Error::protocol(error.to_string()))?;
    Ok(uuids
        .iter()
        .any(|uuid| uuid.eq_ignore_ascii_case(service_uuid())))
}
