#![expect(
    clippy::expect_used,
    reason = "The private D-Bus fake fails the contract on setup errors"
)]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::collections::HashMap;
use std::io::{BufRead as _, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use zbus::blocking::connection::Builder;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{fdo, interface};

pub(crate) struct FakeNetworkManager {
    address: String,
    child: Child,
    profiles: Arc<AtomicUsize>,
    _connection: zbus::blocking::Connection,
}

struct Manager {
    profiles: Arc<AtomicUsize>,
}

struct WifiDevice;

struct P2pDeviceBase;
struct P2pDeviceControl;

struct Peer;
struct Profile {
    profiles: Arc<AtomicUsize>,
}
struct Active {
    activate: Arc<AtomicBool>,
}
struct Ip4;

fn path(value: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(value).expect("NetworkManager object path")
}

impl FakeNetworkManager {
    pub(crate) fn start(activate: bool) -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("private dbus-daemon should start");
        let mut address = String::new();
        let _read =
            BufReader::new(child.stdout.take().expect("dbus address pipe"))
                .read_line(&mut address)
                .expect("dbus address");
        let address = address.trim().to_owned();
        let activate = Arc::new(AtomicBool::new(activate));
        let profiles = Arc::new(AtomicUsize::new(0));
        let connection = Builder::address(address.as_str())
            .expect("dbus address")
            .name("org.freedesktop.NetworkManager")
            .expect("request NetworkManager name")
            .serve_at(
                "/org/freedesktop/NetworkManager",
                Manager {
                    profiles: Arc::clone(&profiles),
                },
            )
            .expect("serve manager")
            .serve_at("/org/freedesktop/NetworkManager/Devices/2", WifiDevice)
            .expect("serve wifi device")
            .serve_at(
                "/org/freedesktop/NetworkManager/Devices/30",
                P2pDeviceBase,
            )
            .expect("serve p2p device")
            .serve_at(
                "/org/freedesktop/NetworkManager/Devices/30",
                P2pDeviceControl,
            )
            .expect("serve p2p control")
            .serve_at("/org/freedesktop/NetworkManager/WifiP2PPeer/1", Peer)
            .expect("serve peer")
            .serve_at(
                "/org/freedesktop/NetworkManager/Settings/1",
                Profile {
                    profiles: Arc::clone(&profiles),
                },
            )
            .expect("serve profile")
            .serve_at(
                "/org/freedesktop/NetworkManager/ActiveConnection/1",
                Active { activate },
            )
            .expect("serve active connection")
            .serve_at("/org/freedesktop/NetworkManager/IP4Config/1", Ip4)
            .expect("serve ip4")
            .build()
            .expect("fake NetworkManager should own the private bus");
        Self {
            address,
            child,
            profiles,
            _connection: connection,
        }
    }

    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn leftover_profiles(&self) -> usize {
        self.profiles.load(Ordering::SeqCst)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for FakeNetworkManager {
    fn drop(&mut self) {
        let _killed = self.child.kill();
        let _waited = self.child.wait();
    }
}

#[interface(name = "org.freedesktop.NetworkManager")]
impl Manager {
    fn get_devices(&self) -> Vec<OwnedObjectPath> {
        vec![
            path("/org/freedesktop/NetworkManager/Devices/2"),
            path("/org/freedesktop/NetworkManager/Devices/30"),
        ]
    }

    fn add_and_activate_connection(
        &self,
        _connection: HashMap<String, HashMap<String, OwnedValue>>,
        _device: OwnedObjectPath,
        _specific_object: OwnedObjectPath,
    ) -> (OwnedObjectPath, OwnedObjectPath) {
        let _previous = self.profiles.fetch_add(1, Ordering::SeqCst);
        (
            path("/org/freedesktop/NetworkManager/Settings/1"),
            path("/org/freedesktop/NetworkManager/ActiveConnection/1"),
        )
    }

    fn deactivate_connection(&self, _active: OwnedObjectPath) {}
}

#[interface(name = "org.freedesktop.NetworkManager.Device")]
impl WifiDevice {
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        2
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Device")]
impl P2pDeviceBase {
    #[zbus(property)]
    fn device_type(&self) -> u32 {
        30
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Device.WifiP2P")]
impl P2pDeviceControl {
    fn start_find(
        &self,
        _options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<()> {
        Ok(())
    }

    fn stop_find(&self) -> fdo::Result<()> {
        Ok(())
    }

    #[zbus(property)]
    fn peers(&self) -> Vec<OwnedObjectPath> {
        vec![path("/org/freedesktop/NetworkManager/WifiP2PPeer/1")]
    }
}

#[interface(name = "org.freedesktop.NetworkManager.WifiP2PPeer")]
impl Peer {
    #[zbus(property)]
    fn name(&self) -> String {
        String::from("peer")
    }

    #[zbus(property)]
    fn hw_address(&self) -> String {
        String::from("AA:BB:CC:DD:EE:FF")
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl Profile {
    fn delete(&self) {
        let _updated = self.profiles.try_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |count| Some(count.saturating_sub(1)),
        );
    }
}

#[interface(name = "org.freedesktop.NetworkManager.Connection.Active")]
impl Active {
    #[zbus(property)]
    fn state(&self) -> u32 {
        if self.activate.load(Ordering::SeqCst) {
            2
        } else {
            1
        }
    }

    #[zbus(property)]
    fn ip4_config(&self) -> OwnedObjectPath {
        path("/org/freedesktop/NetworkManager/IP4Config/1")
    }
}

#[interface(name = "org.freedesktop.NetworkManager.IP4Config")]
impl Ip4 {
    #[zbus(property)]
    fn address_data(&self) -> Vec<HashMap<String, OwnedValue>> {
        let address =
            OwnedValue::try_from(zbus::zvariant::Value::from("10.42.0.2"))
                .expect("address variant");
        vec![HashMap::from([(String::from("address"), address)])]
    }
}
