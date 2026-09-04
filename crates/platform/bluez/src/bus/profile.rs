//! BlueZ `Profile1` Unix-fd acquisition.

use std::collections::HashMap;
use std::os::fd::AsFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use zbus::zvariant::{Fd, ObjectPath, OwnedObjectPath, Value};

use super::sockets::IncomingSockets;
use super::{DbusBytePipe, DbusSession, consume};
use crate::radio::{Address, Error};

/// Exported `org.bluez.Profile1` object that keeps NewConnection fds.
pub(super) struct BluezProfile {
    /// Shared inbox for this registration.
    incoming: Arc<IncomingSockets>,
}

#[zbus::interface(name = "org.bluez.Profile1")]
impl BluezProfile {
    /// Releases the profile when BlueZ unregisters it.
    fn release(&self) -> zbus::fdo::Result<()> {
        Ok(())
    }

    /// Accepts a connected socket from BlueZ.
    fn new_connection(
        &self,
        _device: ObjectPath<'_>,
        fd: Fd<'_>,
        _fd_properties: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        let owned = fd
            .as_fd()
            .try_clone_to_owned()
            .map_err(|error| zbus::fdo::Error::IOError(error.to_string()))?;
        self.incoming
            .push(owned)
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
        Ok(())
    }

    /// Acknowledges a BlueZ disconnect request.
    fn request_disconnection(
        &self,
        _device: ObjectPath<'_>,
    ) -> zbus::fdo::Result<()> {
        Ok(())
    }
}

impl DbusSession {
    /// Registers a profile and returns its path plus fd inbox.
    pub(super) fn register_profile_queue(
        &self,
        uuid: &str,
        psm: Option<u16>,
        role: &str,
    ) -> Result<(OwnedObjectPath, Arc<IncomingSockets>), Error> {
        let path = self.next_path("profile")?;
        let incoming = Arc::new(IncomingSockets::new());
        consume(self.connection.object_server().at(
            &path,
            BluezProfile {
                incoming: Arc::clone(&incoming),
            },
        ))?;
        let manager = zbus::blocking::Proxy::new(
            &self.connection,
            "org.bluez",
            "/org/bluez",
            "org.bluez.ProfileManager1",
        )
        .map_err(|error| Error::bus(error.to_string()))?;
        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        if let Some(psm) = psm {
            let _previous = options.insert("PSM", Value::from(psm));
        }
        let _role = options.insert("Role", Value::from(role));
        consume(
            manager
                .call_method("RegisterProfile", &(path.clone(), uuid, options)),
        )?;
        Ok((path, incoming))
    }

    /// Connects a profile and returns only after BlueZ hands over a socket.
    pub(super) fn connect_device_profile(
        &self,
        address: Address,
        uuid: &str,
        deadline: Duration,
    ) -> Result<DbusBytePipe, Error> {
        let deadline_at = Instant::now()
            .checked_add(deadline)
            .ok_or_else(|| Error::protocol("deadline overflow"))?;
        if Instant::now() >= deadline_at {
            return Err(Error::timeout());
        }
        let (path, incoming) =
            self.register_profile_queue(uuid, None, "client")?;
        let connect_result = self.call_connect_profile(address, uuid);
        let pipe = match connect_result {
            Ok(()) => incoming
                .wait(deadline_at)
                .and_then(DbusBytePipe::from_owned_fd),
            Err(error) => Err(error),
        };
        let _unregistered = self.unregister_profile(&path);
        pipe
    }

    fn call_connect_profile(
        &self,
        address: Address,
        uuid: &str,
    ) -> Result<(), Error> {
        let path = super::objects::device_path(&self.adapter_path, address)?;
        let device = zbus::blocking::Proxy::new(
            &self.connection,
            "org.bluez",
            path,
            "org.bluez.Device1",
        )
        .map_err(|error| Error::bus(error.to_string()))?;
        consume(device.call_method("ConnectProfile", &(uuid,)))
    }
}
