//! Connection, discovery, and visibility composition.

use core::net::SocketAddrV4;
use core::time::Duration;
use std::{fs, io, sync::LazyLock};

use quickshare_bluez::{
    Adapter, BleCandidate, ClassicCandidate, QUICK_SHARE_BLE_UUID,
    ReceiverAdvertisement,
};
use quickshare_connections::{
    Connection, ConnectionIo, ConnectionOptions, Medium,
};
use quickshare_crypto::Handshake;
use quickshare_network::lan::connect as connect_lan;
use quickshare_sharing::{EndpointInfo, ProtocolError, SharingSession};
use rand_core::{OsRng, RngCore as _};

use crate::{
    config::Config,
    daemon::observations::{
        BLE, BLUETOOTH, WIFI_DIRECT, WIFI_HOTSPOT, WIFI_LAN, protocol_reason,
    },
};

/// Connections endpoint identifier used by every SharingSession identity.
pub(crate) const ENDPOINT_ID: &str = "OQSR";
/// Four-byte form of [`ENDPOINT_ID`] used by LAN instance labels.
pub(crate) const ENDPOINT_ID_BYTES: [u8; 4] = *b"OQSR";
/// Fallback used when the host does not expose a usable system hostname.
const FALLBACK_ENDPOINT_NAME: &str = "Omarchy";
/// Google advertisement value for a laptop-class endpoint.
const LAPTOP_DEVICE_TYPE: u8 = 3;
static ENDPOINT_NAME: LazyLock<String> = LazyLock::new(|| {
    Config::load()
        .ok()
        .and_then(|config| config.device_name)
        .unwrap_or_else(|| {
            normalized_endpoint_name(
                fs::read_to_string("/etc/hostname").ok().as_deref(),
            )
        })
});

/// Bluetooth connect budget after a candidate is already stored.
const BLUETOOTH_CONNECT: Duration = Duration::from_secs(8);

/// Ordered media used for one outbound attempt chain.
#[must_use]
pub(crate) const fn attempt_order() -> [Medium; 5] {
    [
        Medium::WifiLan,
        Medium::WifiHotspot,
        Medium::WifiDirect,
        Medium::Ble,
        Medium::Bluetooth,
    ]
}

/// Public snake_case name for a Connections medium.
#[must_use]
pub(crate) const fn medium_name(medium: Medium) -> &'static str {
    match medium {
        Medium::Ble => BLE,
        Medium::Bluetooth => BLUETOOTH,
        Medium::WifiLan => WIFI_LAN,
        Medium::WifiHotspot => WIFI_HOTSPOT,
        Medium::WifiDirect => WIFI_DIRECT,
    }
}

/// A usable private route discovered for one peer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PeerRoute {
    /// BLE GATT weave candidate.
    Ble(BleCandidate),
    /// Classic RFCOMM candidate.
    Classic(ClassicCandidate),
    /// LAN TCP address advertised through DNS-SD.
    Lan(SocketAddrV4),
}

impl PeerRoute {
    /// Returns the Connections medium for this route.
    #[must_use]
    pub(crate) const fn medium(&self) -> Medium {
        match self {
            Self::Ble(_) => Medium::Ble,
            Self::Classic(_) => Medium::Bluetooth,
            Self::Lan(_) => Medium::WifiLan,
        }
    }
}

/// A discovered peer with one private candidate route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PeerSighting {
    /// User-visible name.
    pub name: String,
    /// Stable local peer identifier.
    pub peer_id: String,
    /// Connectable route.
    pub route: PeerRoute,
}

/// Opens an initiator Connections relationship over any byte stream.
///
/// # Errors
///
/// Returns an error when the handshake or framing fails.
pub(crate) fn connect_connection<Stream>(
    stream: Stream,
    medium: Medium,
) -> Result<Connection, ProtocolError>
where
    Stream: ConnectionIo + 'static,
{
    open_connection(stream, medium, ConnectionRole::Initiator)
}

/// Opens a responder Connections relationship over any byte stream.
///
/// # Errors
///
/// Returns an error when the handshake or framing fails.
pub(crate) fn accept_connection<Stream>(
    stream: Stream,
    medium: Medium,
) -> Result<Connection, ProtocolError>
where
    Stream: ConnectionIo + 'static,
{
    open_connection(stream, medium, ConnectionRole::Responder)
}
#[derive(Clone, Copy)]
enum ConnectionRole {
    Initiator,
    Responder,
}

fn open_connection<Stream>(
    stream: Stream,
    medium: Medium,
    role: ConnectionRole,
) -> Result<Connection, ProtocolError>
where
    Stream: ConnectionIo + 'static,
{
    let result = (|| {
        let mut rng = OsRng;
        let options = connection_options(&mut rng, medium)?;
        let connection = match role {
            ConnectionRole::Initiator => Connection::connect_io(
                stream,
                Handshake::initiator_with_rng(&mut rng),
                options,
            )?,
            ConnectionRole::Responder => Connection::accept_io(
                stream,
                Handshake::responder_with_rng(&mut rng),
                options,
            )?,
        };
        Ok(connection)
    })();
    trace_handshake(medium, result)
}

/// Connects a stored candidate and returns an encrypted connection.
///
/// # Errors
///
/// Returns a typed failure when the route cannot carry SharingSession.
pub(crate) fn connect_route(
    adapter: Option<&Adapter>,
    route: &PeerRoute,
) -> Result<Connection, ProtocolError> {
    match route {
        PeerRoute::Lan(address) => {
            let stream = match connect_lan(*address) {
                Ok(stream) => stream,
                Err(error) => {
                    tracing::warn!(
                        stage = "connection",
                        medium = WIFI_LAN,
                        error_class = "io",
                        "connection failed"
                    );
                    return Err(ProtocolError::Io(error));
                }
            };
            connect_connection(stream, Medium::WifiLan)
        }
        PeerRoute::Ble(candidate) => {
            let Some(adapter) = adapter else {
                tracing::warn!(
                    stage = "connection",
                    medium = BLE,
                    available = false,
                    error_class = "unavailable",
                    "connection failed"
                );
                return Err(missing_adapter());
            };
            let io = match adapter
                .connect_gatt_weave(candidate, BLUETOOTH_CONNECT)
                .and_then(quickshare_bluez::WeaveSocket::into_io)
            {
                Ok(io) => io,
                Err(error) => {
                    tracing::warn!(
                        stage = "connection",
                        medium = BLE,
                        error_class = "io",
                        "connection failed"
                    );
                    return Err(bluetooth_error(error));
                }
            };
            connect_connection(io, Medium::Ble)
        }
        PeerRoute::Classic(candidate) => {
            let Some(adapter) = adapter else {
                tracing::warn!(
                    stage = "connection",
                    medium = BLUETOOTH,
                    available = false,
                    error_class = "unavailable",
                    "connection failed"
                );
                return Err(missing_adapter());
            };
            let io = match adapter
                .connect_classic(
                    candidate,
                    QUICK_SHARE_BLE_UUID,
                    BLUETOOTH_CONNECT,
                )
                .and_then(quickshare_bluez::ClassicSocket::into_io)
            {
                Ok(io) => io,
                Err(error) => {
                    tracing::warn!(
                        stage = "connection",
                        medium = BLUETOOTH,
                        error_class = "io",
                        "connection failed"
                    );
                    return Err(bluetooth_error(error));
                }
            };
            connect_connection(io, Medium::Bluetooth)
        }
    }
}

/// Wraps an encrypted connection as a Sharing session.
#[must_use]
pub(crate) fn sharing_session(connection: Connection) -> SharingSession {
    SharingSession::new(connection)
}

/// Starts BLE and Classic visibility leases when BlueZ is present.
#[must_use]
pub(crate) fn open_visibility(adapter: Option<&Adapter>) -> VisibilityLeases {
    let Some(adapter) = adapter else {
        return VisibilityLeases::default();
    };
    let advertisement = optional_lease(
        "ble_advertise",
        adapter.advertise_receiver(ReceiverAdvertisement::new(
            ENDPOINT_ID.as_bytes().to_vec(),
        )),
    );
    let classic = optional_lease(
        "classic_listen",
        adapter.listen_classic(QUICK_SHARE_BLE_UUID),
    );
    let gatt = optional_lease("gatt_weave", adapter.serve_gatt_weave());
    let l2cap = optional_lease("l2cap_listen", adapter.listen_l2cap(0x1001));
    VisibilityLeases {
        advertisement,
        classic,
        gatt,
        l2cap,
    }
}

/// Starts BLE and Classic discovery leases when BlueZ is present.
#[must_use]
pub(crate) fn start_discovery(
    adapter: Option<&Adapter>,
    deadline: Duration,
) -> DiscoveryLeases {
    let Some(adapter) = adapter else {
        return DiscoveryLeases::default();
    };
    DiscoveryLeases {
        ble: optional_lease("ble_scan", adapter.scan_ble(deadline)),
        classic: optional_lease(
            "classic_scan",
            adapter.discover_classic(deadline),
        ),
    }
}

/// RAII visibility handles released on drop.
#[derive(Debug, Default)]
pub(crate) struct VisibilityLeases {
    advertisement: Option<quickshare_bluez::BleAdvertisement>,
    classic: Option<quickshare_bluez::ClassicListener>,
    gatt: Option<quickshare_bluez::GattWeaveServer>,
    l2cap: Option<quickshare_bluez::L2capListener>,
}

/// RAII discovery handles released on drop.
#[derive(Debug, Default)]
pub(crate) struct DiscoveryLeases {
    ble: Option<quickshare_bluez::BleScan>,
    classic: Option<quickshare_bluez::ClassicDiscovery>,
}

impl VisibilityLeases {
    pub(crate) fn close(self) {
        drop(self);
    }

    /// Accepts one pending Bluetooth inbound stream.
    pub(crate) fn accept_next(
        &mut self,
    ) -> Option<(quickshare_bluez::BluetoothIo, Medium)> {
        let _advertising = self.advertisement.is_some();
        if let Some(server) = self.gatt.as_mut()
            && let Ok(Some(socket)) = server.accept()
            && let Ok(io) = socket.into_io()
        {
            return Some((io, Medium::Ble));
        }
        if let Some(listener) = self.classic.as_mut()
            && let Ok(Some(socket)) = listener.accept()
            && let Ok(io) = socket.into_io()
        {
            return Some((io, Medium::Bluetooth));
        }
        if let Some(listener) = self.l2cap.as_mut()
            && let Ok(Some(channel)) = listener.accept()
            && let Ok(io) = channel.into_io()
        {
            return Some((io, Medium::Bluetooth));
        }
        None
    }
}

impl DiscoveryLeases {
    pub(crate) fn close(self) {
        drop(self);
    }

    /// Returns the next BLE or Classic Quick Share candidate.
    pub(crate) fn next_sighting(&mut self) -> Option<PeerSighting> {
        if let Some(scan) = self.ble.as_mut()
            && let Ok(Some(candidate)) = scan.next_candidate()
        {
            let peer_id = ble_peer_id(&candidate);
            return Some(PeerSighting {
                name: peer_id.clone(),
                peer_id,
                route: PeerRoute::Ble(candidate),
            });
        }
        if let Some(discovery) = self.classic.as_mut()
            && let Ok(Some(candidate)) = discovery.next_candidate()
        {
            let peer_id = candidate.address().to_string();
            return Some(PeerSighting {
                name: peer_id.clone(),
                peer_id,
                route: PeerRoute::Classic(candidate),
            });
        }
        None
    }
}

fn ble_peer_id(candidate: &BleCandidate) -> String {
    let data = candidate.service_data();
    if data.len() == 4 && data.iter().all(u8::is_ascii_alphanumeric) {
        String::from_utf8_lossy(data).into_owned()
    } else {
        candidate.address().to_string()
    }
}

fn connection_options(
    rng: &mut OsRng,
    medium: Medium,
) -> Result<ConnectionOptions, ProtocolError> {
    let mut salt = [0; 2];
    let mut metadata_key = [0; 14];
    rng.fill_bytes(&mut salt);
    rng.fill_bytes(&mut metadata_key);
    let endpoint_name = endpoint_name();
    let endpoint_info = EndpointInfo::new(
        0,
        LAPTOP_DEVICE_TYPE,
        salt,
        metadata_key,
        Some(endpoint_name),
        None,
        Vec::new(),
    )?
    .encode();
    Ok(ConnectionOptions::new(ENDPOINT_ID, endpoint_name)
        .with_endpoint_info(endpoint_info)
        .with_medium(medium))
}

/// Returns the configured device name or system hostname shown to nearby peers.
pub(crate) fn endpoint_name() -> &'static str {
    &ENDPOINT_NAME
}

fn normalized_endpoint_name(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|name| !name.is_empty() && u8::try_from(name.len()).is_ok())
        .map_or_else(|| String::from(FALLBACK_ENDPOINT_NAME), String::from)
}

fn missing_adapter() -> ProtocolError {
    ProtocolError::Io(io::Error::new(
        io::ErrorKind::NotFound,
        "bluetooth adapter is missing",
    ))
}

fn optional_lease<T, E>(
    stage: &'static str,
    result: Result<T, E>,
) -> Option<T> {
    result.map_or_else(
        |_| {
            tracing::warn!(stage, available = false, "adapter stage failed");
            None
        },
        |value| {
            tracing::debug!(stage, available = true, "adapter stage ready");
            Some(value)
        },
    )
}

fn trace_handshake<T>(
    medium: Medium,
    result: Result<T, ProtocolError>,
) -> Result<T, ProtocolError> {
    match result {
        Ok(value) => {
            tracing::debug!(
                stage = "handshake",
                medium = medium_name(medium),
                "adapter stage ready"
            );
            Ok(value)
        }
        Err(error) => {
            tracing::warn!(
                stage = "handshake",
                medium = medium_name(medium),
                error_class = protocol_reason(&error),
                "handshake failed"
            );
            Err(error)
        }
    }
}

fn bluetooth_error(error: quickshare_bluez::Error) -> ProtocolError {
    ProtocolError::Io(io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::normalized_endpoint_name;

    #[test]
    fn endpoint_name_uses_trimmed_hostname_or_fallback() {
        assert_eq!(
            normalized_endpoint_name(Some("omarchy-macbook\n")),
            "omarchy-macbook"
        );
        assert_eq!(normalized_endpoint_name(Some(" \n")), "Omarchy");
        assert_eq!(normalized_endpoint_name(None), "Omarchy");
    }
}
