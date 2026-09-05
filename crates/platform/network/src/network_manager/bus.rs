use core::fmt;
use core::net::Ipv4Addr;
use core::time::Duration;
use std::collections::HashMap;
use std::str::FromStr as _;
use std::time::Instant;

use zbus::blocking::Proxy;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::diagnostics::event;
use super::settings::{Settings, p2p, wireless};
use super::{
    Candidate, Credentials, Discovery, Medium, NetworkManager, Peer, Role,
    Session,
};
use crate::Error;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE_IFACE: &str = "org.freedesktop.NetworkManager.Device";
const P2P_IFACE: &str = "org.freedesktop.NetworkManager.Device.WifiP2P";
const PEER_IFACE: &str = "org.freedesktop.NetworkManager.WifiP2PPeer";
const ACTIVE_IFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IP4_IFACE: &str = "org.freedesktop.NetworkManager.IP4Config";
const PROFILE_IFACE: &str =
    "org.freedesktop.NetworkManager.Settings.Connection";
const WIFI: u32 = 2;
const WIFI_P2P: u32 = 30;
const ACTIVATED: u32 = 2;
const POLL: Duration = Duration::from_millis(10);

impl NetworkManager {
    pub(super) fn activate_hotspot(
        &self,
        credentials: &Credentials,
        timeout: Duration,
        role: Role,
    ) -> Result<Session, Error> {
        let (id, mode, ipv4) = match role {
            Role::Client => {
                ("quickshare-hotspot-client", "infrastructure", "auto")
            }
            Role::Owner => ("quickshare-hotspot-owner", "ap", "shared"),
        };
        self.activate(
            wireless(id, mode, credentials, ipv4)?,
            WIFI,
            "/",
            timeout,
            Medium::Hotspot,
            role,
            credentials,
        )
    }

    pub(super) fn start_find(
        &self,
        timeout: Duration,
    ) -> Result<Discovery, Error> {
        event(
            "start_discovery",
            "started",
            "none",
            Some(Medium::WifiDirect),
            None,
        );
        let device = self.find_device(WIFI_P2P)?;
        let mut options = HashMap::new();
        let seconds = u32::try_from(timeout.as_secs().min(u64::from(u32::MAX)))
            .unwrap_or(u32::MAX);
        let _previous_timeout = options
            .insert(String::from("timeout"), owned(Value::from(seconds))?);
        let _start_find_reply = self
            .proxy(device.as_str(), P2P_IFACE)?
            .call_method("StartFind", &options)
            .map_err(nm_error)
            .inspect_err(|_| {
                event(
                    "start_discovery",
                    "failure",
                    "dbus",
                    Some(Medium::WifiDirect),
                    None,
                );
            })?;
        Ok(Discovery {
            device,
            manager: self.clone(),
            stopped: false,
        })
    }

    pub(super) fn activate_wifi_direct(
        &self,
        peer: Option<&Peer>,
        credentials: &Credentials,
        timeout: Duration,
    ) -> Result<Session, Error> {
        let (specific, role) = if let Some(peer) = peer {
            let device = self.find_device(WIFI_P2P)?;
            (self.peer_path(device.as_str(), peer)?, Role::Client)
        } else {
            (
                ObjectPath::try_from("/").map_err(nm_error)?.into(),
                Role::Owner,
            )
        };
        let id = if peer.is_some() {
            "quickshare-p2p-client"
        } else {
            "quickshare-p2p-owner"
        };
        self.activate(
            p2p(id, peer.map(Peer::address), credentials)?,
            WIFI_P2P,
            specific.as_str(),
            timeout,
            Medium::WifiDirect,
            role,
            credentials,
        )
    }

    fn activate(
        &self,
        settings: Settings,
        device_type: u32,
        specific: &str,
        timeout: Duration,
        medium: Medium,
        role: Role,
        credentials: &Credentials,
    ) -> Result<Session, Error> {
        event("activate", "started", "none", Some(medium), Some(role));
        let device = self.find_device(device_type)?;
        let specific = ObjectPath::try_from(specific).map_err(nm_error)?;
        let (profile, active): (OwnedObjectPath, OwnedObjectPath) = self
            .proxy(NM_PATH, NM)?
            .call(
                "AddAndActivateConnection",
                &(settings, device.as_ref(), specific),
            )
            .map_err(nm_error)?;
        let mut session = Session {
            active: Some(active),
            candidate: Candidate {
                addresses: Vec::new(),
                gateway: credentials.gateway,
                medium,
                port: credentials.port,
                role,
            },
            manager: self.clone(),
            profile: Some(profile),
        };
        if let Err(error) =
            self.wait_activated(session.active.as_ref(), timeout)
        {
            event(
                "activate",
                "failure",
                "activation_failed",
                Some(medium),
                Some(role),
            );
            session.cleanup();
            return Err(error);
        }
        session.candidate = self
            .read_candidate(session.active.as_ref(), medium, role, credentials)
            .inspect_err(|_| {
                event(
                    "read_candidate",
                    "failure",
                    "dbus",
                    Some(medium),
                    Some(role),
                );
            })?;
        event("activate", "success", "none", Some(medium), Some(role));
        Ok(session)
    }

    fn wait_activated(
        &self,
        active: Option<&OwnedObjectPath>,
        timeout: Duration,
    ) -> Result<(), Error> {
        let Some(active) = active else {
            return Err(Error(String::from("missing active connection")));
        };
        let started = Instant::now();
        loop {
            let state: u32 = self
                .proxy(active.as_str(), ACTIVE_IFACE)?
                .get_property("State")
                .map_err(nm_error)?;
            if state == ACTIVATED {
                return Ok(());
            }
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                event("activate", "timeout", "deadline", None, None);
                return Err(Error(String::from(
                    "network session timed out before activation",
                )));
            };
            if remaining.is_zero() {
                event("activate", "timeout", "deadline", None, None);
                return Err(Error(String::from(
                    "network session timed out before activation",
                )));
            }
            std::thread::sleep(remaining.min(POLL));
        }
    }

    fn read_candidate(
        &self,
        active: Option<&OwnedObjectPath>,
        medium: Medium,
        role: Role,
        credentials: &Credentials,
    ) -> Result<Candidate, Error> {
        let Some(active) = active else {
            return Err(Error(String::from("missing active connection")));
        };
        let ip4: OwnedObjectPath = self
            .proxy(active.as_str(), ACTIVE_IFACE)?
            .get_property("Ip4Config")
            .map_err(nm_error)?;
        let data: Vec<HashMap<String, OwnedValue>> = self
            .proxy(ip4.as_str(), IP4_IFACE)?
            .get_property("AddressData")
            .map_err(nm_error)?;
        let mut addresses = Vec::new();
        for entry in data {
            if let Some(value) = entry.get("address")
                && let Ok(text) = <&str>::try_from(value)
                && let Ok(address) = Ipv4Addr::from_str(text)
            {
                addresses.push(address);
            }
        }
        Ok(Candidate {
            addresses,
            gateway: credentials.gateway,
            medium,
            port: credentials.port,
            role,
        })
    }

    fn find_device(&self, device_type: u32) -> Result<OwnedObjectPath, Error> {
        let devices: Vec<OwnedObjectPath> = self
            .proxy(NM_PATH, NM)?
            .call("GetDevices", &())
            .map_err(nm_error)?;
        for path in devices {
            let found: u32 = self
                .proxy(path.as_str(), DEVICE_IFACE)?
                .get_property("DeviceType")
                .map_err(nm_error)?;
            if found == device_type {
                return Ok(path);
            }
        }
        Err(Error(String::from("no matching NetworkManager device")))
    }

    fn peer_path(
        &self,
        device: &str,
        peer: &Peer,
    ) -> Result<OwnedObjectPath, Error> {
        let peers: Vec<OwnedObjectPath> = self
            .proxy(device, P2P_IFACE)?
            .get_property("Peers")
            .map_err(nm_error)?;
        for path in peers {
            let address: String = self
                .proxy(path.as_str(), PEER_IFACE)?
                .get_property("HwAddress")
                .map_err(nm_error)?;
            if address.eq_ignore_ascii_case(peer.address()) {
                return Ok(path);
            }
        }
        Err(Error(String::from("wifi direct peer is not visible")))
    }

    fn proxy<'a>(
        &'a self,
        path: &'a str,
        interface: &'a str,
    ) -> Result<Proxy<'a>, Error> {
        Proxy::new(&self.connection, NM, path, interface).map_err(nm_error)
    }

    fn deactivate(&self, active: &OwnedObjectPath) -> Result<(), Error> {
        self.proxy(NM_PATH, NM)?
            .call_method("DeactivateConnection", &(active.as_ref(),))
            .map(|_reply| ())
            .map_err(nm_error)
    }

    fn delete(&self, profile: &OwnedObjectPath) -> Result<(), Error> {
        self.proxy(profile.as_str(), PROFILE_IFACE)?
            .call_method("Delete", &())
            .map(|_reply| ())
            .map_err(nm_error)
    }
}

impl Discovery {
    pub(super) fn wait_peer(
        &self,
        timeout: Duration,
    ) -> Result<Option<Peer>, Error> {
        let started = Instant::now();
        loop {
            if let Some(peer) = self.read_peer()? {
                event(
                    "discover_peer",
                    "success",
                    "none",
                    Some(Medium::WifiDirect),
                    None,
                );
                return Ok(Some(peer));
            }
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                event(
                    "discover_peer",
                    "timeout",
                    "deadline",
                    Some(Medium::WifiDirect),
                    None,
                );
                return Ok(None);
            };
            if remaining.is_zero() {
                event(
                    "discover_peer",
                    "timeout",
                    "deadline",
                    Some(Medium::WifiDirect),
                    None,
                );
                return Ok(None);
            }
            std::thread::sleep(remaining.min(POLL));
        }
    }

    fn read_peer(&self) -> Result<Option<Peer>, Error> {
        let peers: Vec<OwnedObjectPath> = self
            .manager
            .proxy(self.device.as_str(), P2P_IFACE)?
            .get_property("Peers")
            .map_err(nm_error)?;
        let Some(path) = peers.into_iter().next() else {
            return Ok(None);
        };
        let name: String = self
            .manager
            .proxy(path.as_str(), PEER_IFACE)?
            .get_property("Name")
            .map_err(nm_error)?;
        let address: String = self
            .manager
            .proxy(path.as_str(), PEER_IFACE)?
            .get_property("HwAddress")
            .map_err(nm_error)?;
        Ok(Some(Peer::new(address, name)))
    }

    pub(super) fn stop_find(&mut self) -> Result<(), Error> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        self.manager
            .proxy(self.device.as_str(), P2P_IFACE)?
            .call_method("StopFind", &())
            .map(|_reply| ())
            .map_err(nm_error)
            .inspect(|&()| {
                event(
                    "stop_discovery",
                    "success",
                    "none",
                    Some(Medium::WifiDirect),
                    None,
                );
            })
            .inspect_err(|_| {
                event(
                    "stop_discovery",
                    "failure",
                    "dbus",
                    Some(Medium::WifiDirect),
                    None,
                );
            })
    }
}

impl Session {
    pub(super) fn teardown(&mut self) -> Result<(), Error> {
        let medium = self.candidate.medium;
        let role = self.candidate.role;
        let deactivate = self
            .active
            .take()
            .map_or(Ok(()), |active| self.manager.deactivate(&active));
        event(
            "deactivate",
            if deactivate.is_ok() {
                "success"
            } else {
                "failure"
            },
            if deactivate.is_ok() { "none" } else { "dbus" },
            Some(medium),
            Some(role),
        );
        let delete = self
            .profile
            .take()
            .map_or(Ok(()), |profile| self.manager.delete(&profile));
        event(
            "delete_profile",
            if delete.is_ok() { "success" } else { "failure" },
            if delete.is_ok() { "none" } else { "dbus" },
            Some(medium),
            Some(role),
        );
        deactivate.and(delete)
    }

    pub(super) fn cleanup(&mut self) {
        let _result = self.teardown();
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for Session {
    #[inline]
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for Discovery {
    #[inline]
    fn drop(&mut self) {
        let _result = self.stop_find();
    }
}

fn owned(value: Value<'static>) -> Result<OwnedValue, Error> {
    OwnedValue::try_from(value).map_err(|error| Error(error.to_string()))
}

pub(super) fn nm_error<E: fmt::Display>(error: E) -> Error {
    Error(error.to_string())
}
