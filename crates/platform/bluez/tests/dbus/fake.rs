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
use zbus::fdo::{self, ObjectManager};
use zbus::interface;

use quickshare_bluez::QUICK_SHARE_BLE_UUID;

pub(crate) const BLE_ADDRESS: &str = "10:20:30:40:50:60";
pub(crate) const CLASSIC_ADDRESS: &str = "22:33:44:55:66:77";
pub(crate) const OTHER_ADDRESS: &str = "AA:BB:CC:DD:EE:FF";
pub(crate) const MALFORMED_ADDRESS: &str = "12:34:56:78:9A:BC";
pub(crate) const BLE_SERVICE_DATA: &[u8] = &[0x23, 0x0A, 0x0B];
const SERIAL_PORT: &str = "00001101-0000-1000-8000-00805f9b34fb";

pub(crate) struct FakeBluez {
    address: String,
    child: Child,
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    connection: zbus::blocking::Connection,
}

struct Adapter {
    discovering: Arc<AtomicBool>,
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

struct BleDevice;
struct ClassicDevice;
struct OtherDevice;
struct MalformedDevice;

impl FakeBluez {
    pub(crate) fn start() -> Self {
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
        let discovering = Arc::new(AtomicBool::new(false));
        let starts = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let connection = Builder::address(address.as_str())
            .expect("dbus address")
            .name("org.bluez")
            .expect("request org.bluez")
            .serve_at(
                "/org/bluez/hci0",
                Adapter {
                    discovering: Arc::clone(&discovering),
                    starts: Arc::clone(&starts),
                    stops: Arc::clone(&stops),
                },
            )
            .expect("serve adapter")
            .serve_at("/org/bluez/hci0/dev_10_20_30_40_50_60", BleDevice)
            .expect("serve BLE device")
            .serve_at("/org/bluez/hci0/dev_22_33_44_55_66_77", ClassicDevice)
            .expect("serve Classic device")
            .serve_at("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF", OtherDevice)
            .expect("serve unrelated device")
            .serve_at("/", ObjectManager)
            .expect("serve object manager")
            .build()
            .expect("fake BlueZ should own the private bus");
        Self {
            address,
            child,
            starts,
            stops,
            connection,
        }
    }

    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn add_malformed_device(&self) {
        let _added = self
            .connection
            .object_server()
            .at("/org/bluez/hci0/dev_12_34_56_78_9A_BC", MalformedDevice)
            .expect("serve malformed device");
    }

    pub(crate) fn start_count(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub(crate) fn stop_count(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for FakeBluez {
    fn drop(&mut self) {
        let _killed = self.child.kill();
        let _waited = self.child.wait();
    }
}

#[interface(name = "org.bluez.Adapter1")]
impl Adapter {
    #[zbus(property, name = "Powered")]
    fn powered(&self) -> bool {
        true
    }

    fn set_discovery_filter(
        &self,
        _filter: HashMap<String, zbus::zvariant::OwnedValue>,
    ) -> fdo::Result<()> {
        Ok(())
    }

    fn start_discovery(&self) -> fdo::Result<()> {
        if self.discovering.swap(true, Ordering::SeqCst) {
            return Err(fdo::Error::Failed(String::from("InProgress")));
        }
        let _count = self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn stop_discovery(&self) -> fdo::Result<()> {
        if !self.discovering.swap(false, Ordering::SeqCst) {
            return Err(fdo::Error::Failed(String::from("NotDiscovering")));
        }
        let _count = self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[interface(name = "org.bluez.Device1")]
impl BleDevice {
    #[zbus(property, name = "Address")]
    fn address(&self) -> &str {
        BLE_ADDRESS
    }

    #[zbus(property, name = "UUIDs")]
    fn uuids(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property, name = "ServiceData")]
    fn service_data(&self) -> HashMap<String, Vec<u8>> {
        let mut data = HashMap::new();
        let _previous = data.insert(
            String::from(QUICK_SHARE_BLE_UUID),
            BLE_SERVICE_DATA.to_vec(),
        );
        data
    }
}

#[interface(name = "org.bluez.Device1")]
impl ClassicDevice {
    #[zbus(property, name = "Address")]
    fn address(&self) -> &str {
        CLASSIC_ADDRESS
    }

    #[zbus(property, name = "UUIDs")]
    fn uuids(&self) -> Vec<String> {
        vec![String::from(QUICK_SHARE_BLE_UUID)]
    }

    #[zbus(property, name = "ServiceData")]
    fn service_data(&self) -> HashMap<String, Vec<u8>> {
        HashMap::new()
    }
}

#[interface(name = "org.bluez.Device1")]
impl OtherDevice {
    #[zbus(property, name = "Address")]
    fn address(&self) -> &str {
        OTHER_ADDRESS
    }

    #[zbus(property, name = "UUIDs")]
    fn uuids(&self) -> Vec<String> {
        vec![String::from(SERIAL_PORT)]
    }

    #[zbus(property, name = "ServiceData")]
    fn service_data(&self) -> HashMap<String, Vec<u8>> {
        HashMap::new()
    }
}

#[interface(name = "org.bluez.Device1")]
impl MalformedDevice {
    #[zbus(property, name = "Address")]
    fn address(&self) -> &str {
        MALFORMED_ADDRESS
    }

    #[zbus(property, name = "UUIDs")]
    fn uuids(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property, name = "ServiceData")]
    fn service_data(&self) -> &'static str {
        "not-a-service-data-map"
    }
}
