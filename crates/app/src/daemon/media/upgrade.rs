//! Bandwidth-upgrade composition over an existing Connections session.

use core::net::SocketAddrV4;
use core::time::Duration;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Instant;

use quickshare_connections::{
    Connection, ConnectionIo, Event, Medium, UpgradeCredentials,
    UpgradeDecision, UpgradeEvent,
};
use quickshare_network::lan::connect as connect_lan;
use quickshare_network::network_manager::{
    Credentials as HotspotCredentials, NetworkManager, Session as WifiSession,
};
use quickshare_sharing::ProtocolError;

use crate::daemon::observations::{
    io_error_kind, protocol_io_kind, protocol_reason,
};

use super::{attempt_order, endpoint_name, medium_name};

/// Deadline used to join or own an upgraded Wi-Fi medium.
const UPGRADE_DEADLINE: Duration = Duration::from_secs(8);
/// Poll interval while waiting for a peer to join an upgrade listener.
const UPGRADE_ACCEPT_POLL: Duration = Duration::from_millis(50);

/// Decides whether to stay, upgrade, or fall back between two media.
#[must_use]
pub(crate) const fn upgrade_decision(
    current: Medium,
    offered: Medium,
) -> UpgradeDecision {
    UpgradeDecision::from_media(current, offered)
}

/// Completes an upgrade onto `stream` or reports failure and keeps the
/// original.
///
/// # Errors
///
/// Returns an error when the Connections upgrade frame cannot be written.
pub(crate) fn complete_or_fail_upgrade<Stream>(
    connection: &mut Connection,
    medium: Medium,
    upgraded: Result<Stream, ProtocolError>,
) -> Result<(), ProtocolError>
where
    Stream: ConnectionIo + 'static,
{
    match upgraded {
        Ok(stream) => {
            if let Err(error) = connection.complete_upgrade_io(medium, stream) {
                let error = ProtocolError::Connection(error);
                tracing::warn!(
                    target: "omarchy_quickshare::protocol",
                    stage = "upgrade",
                    operation = "complete",
                    outcome = "failed",
                    medium = medium_name(medium),
                    reason = crate::daemon::observations::pairing_error_class(
                        &error,
                    ),
                    io_error_kind = protocol_io_kind(&error).unwrap_or("none"),
                    "upgrade failed"
                );
                connection.fail_upgrade(medium)?;
            } else {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "upgrade",
                    operation = "complete",
                    outcome = "completed",
                    medium = medium_name(medium),
                    "adapter stage ready"
                );
            }
            Ok(())
        }
        Err(error) => {
            tracing::warn!(
                target: "omarchy_quickshare::protocol",
                stage = "upgrade",
                operation = "complete",
                outcome = "failed",
                medium = medium_name(medium),
                reason = protocol_reason(&error),
                io_error_kind = protocol_io_kind(&error),
                "upgrade failed"
            );
            connection.fail_upgrade(medium)?;
            Ok(())
        }
    }
}

/// Initiator offers a hosted hotspot or Wi-Fi Direct path, then switches or
/// falls back.
///
/// # Errors
///
/// Returns an error when encryption or the original connection fails.
pub(crate) fn initiate_bandwidth_upgrade(
    connection: &mut Connection,
    manager: Option<&NetworkManager>,
) -> Result<Option<WifiSession>, ProtocolError> {
    let current = connection.medium();
    if current.rank() >= Medium::WifiLan.rank() {
        return Ok(None);
    }
    let Some(manager) = manager else {
        tracing::warn!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "offer",
            outcome = "unavailable",
            available = false,
            error_class = "unavailable",
            "upgrade failed"
        );
        connection.fail_upgrade(Medium::WifiHotspot)?;
        return Ok(None);
    };
    let listener = TcpListener::bind("0.0.0.0:0")?;
    let port = listener.local_addr()?.port();
    let credentials = HotspotCredentials::new(
        String::from("DIRECT-OQSR"),
        String::from("quickshare"),
    )
    .with_port(port);
    for medium in attempt_order() {
        if upgrade_decision(current, medium) != UpgradeDecision::Upgrade(medium)
        {
            continue;
        }
        let started = match medium {
            Medium::WifiHotspot => {
                manager.start_hotspot(&credentials, UPGRADE_DEADLINE)
            }
            Medium::WifiDirect => {
                manager.start_wifi_direct(&credentials, UPGRADE_DEADLINE)
            }
            Medium::Ble | Medium::Bluetooth | Medium::WifiLan => continue,
        };
        match started {
            Ok(session) => {
                offer_hosted_path(connection, &session, port)?;
                match wait_for_upgrade_stream(&listener) {
                    Ok(stream) => {
                        match connection.complete_upgrade_io(medium, stream) {
                            Ok(()) => {
                                tracing::debug!(
                                    target: "omarchy_quickshare::protocol",
                                    stage = "upgrade",
                                    operation = "complete",
                                    outcome = "completed",
                                    medium = medium_name(medium),
                                    "adapter stage ready"
                                );
                                return Ok(Some(session));
                            }
                            Err(error) => {
                                let error = ProtocolError::Connection(error);
                                tracing::warn!(
                                    target: "omarchy_quickshare::protocol",
                                    stage = "upgrade",
                                    operation = "complete",
                                    outcome = "failed",
                                    medium = medium_name(medium),
                                    reason =
                                        crate::daemon::observations::
                                            pairing_error_class(&error),
                                    io_error_kind = protocol_io_kind(&error)
                                        .unwrap_or("none"),
                                    "upgrade failed"
                                );
                                connection.fail_upgrade(medium)?;
                                drop(session);
                            }
                        }
                    }
                    Err(error) => {
                        let reason = if error.kind() == io::ErrorKind::TimedOut
                        {
                            "timed_out"
                        } else {
                            "io"
                        };
                        tracing::warn!(
                            target: "omarchy_quickshare::protocol",
                            stage = "upgrade",
                            operation = "accept",
                            outcome = "failed",
                            medium = medium_name(medium),
                            reason,
                            io_error_kind = io_error_kind(&error),
                            "upgrade failed"
                        );
                        connection.fail_upgrade(medium)?;
                        drop(session);
                    }
                }
            }
            Err(_) => {
                tracing::warn!(
                    target: "omarchy_quickshare::protocol",
                    stage = "upgrade",
                    operation = "open_medium",
                    outcome = "failed",
                    medium = medium_name(medium),
                    error_class = "unavailable",
                    "upgrade failed"
                );
            }
        }
    }
    connection.fail_upgrade(Medium::WifiHotspot)?;
    Ok(None)
}

/// Responder joins an offered hotspot or Wi-Fi Direct path, or stays on the
/// original.
///
/// # Errors
///
/// Returns an error when encryption fails on the original connection.
pub(crate) fn accept_bandwidth_upgrade(
    connection: &mut Connection,
    event: UpgradeEvent,
    manager: Option<&NetworkManager>,
) -> Result<Option<WifiSession>, ProtocolError> {
    let UpgradeEvent::PathAvailable {
        medium,
        credentials,
    } = event
    else {
        return Ok(None);
    };
    if upgrade_decision(connection.medium(), medium)
        != UpgradeDecision::Upgrade(medium)
    {
        connection.fail_upgrade(medium)?;
        return Ok(None);
    }
    match join_offered_path(manager, medium, &credentials) {
        Ok((stream, session)) => {
            complete_or_fail_upgrade(connection, medium, Ok(stream))?;
            Ok(session)
        }
        Err(error) => {
            tracing::warn!(
                target: "omarchy_quickshare::protocol",
                stage = "upgrade",
                operation = "join",
                outcome = "failed",
                medium = medium_name(medium),
                error_class = protocol_reason(&error),
                io_error_kind = protocol_io_kind(&error),
                "upgrade failed"
            );
            complete_or_fail_upgrade::<TcpStream>(
                connection,
                medium,
                Err(error),
            )?;
            Ok(None)
        }
    }
}

/// Drives a received upgrade offer before payload transfer.
///
/// # Errors
pub(crate) fn accept_negotiated_upgrade(
    connection: &mut Connection,
    manager: Option<&NetworkManager>,
) -> Result<Option<WifiSession>, ProtocolError> {
    if connection.medium().rank() >= Medium::WifiLan.rank() {
        return Ok(None);
    }
    loop {
        match connection.receive()? {
            Event::Upgrade {
                event: event @ UpgradeEvent::PathAvailable { .. },
            } => {
                if let Some(session) =
                    accept_bandwidth_upgrade(connection, event, manager)?
                {
                    return Ok(Some(session));
                }
                if connection.medium().rank() >= Medium::WifiLan.rank() {
                    return Ok(None);
                }
            }
            Event::Upgrade { .. } | Event::KeepAlive { .. } => {}
            Event::Disconnected => return Err(ProtocolError::Disconnected),
            event => {
                connection.unread(event);
                return Ok(None);
            }
        }
    }
}

fn offer_hosted_path(
    connection: &mut Connection,
    session: &WifiSession,
    port: u16,
) -> Result<(), ProtocolError> {
    let candidate = session.candidate();
    let ip_address = candidate.addresses().first().copied();
    let gateway = candidate.gateway().or(ip_address);
    let credentials = UpgradeCredentials {
        frequency: None,
        gateway,
        ip_address,
        password: Some(String::from("quickshare")),
        port: Some(port),
        ssid: Some(String::from("DIRECT-OQSR")),
        device_name: Some(String::from(endpoint_name())),
        pin: None,
    };
    let medium = match candidate.medium() {
        quickshare_network::Medium::Hotspot => Medium::WifiHotspot,
        quickshare_network::Medium::WifiDirect => Medium::WifiDirect,
        quickshare_network::Medium::Lan => Medium::WifiLan,
        _ => Medium::WifiLan,
    };
    connection.propose_upgrade_path(medium, &credentials)?;
    Ok(())
}

fn join_offered_path(
    manager: Option<&NetworkManager>,
    medium: Medium,
    credentials: &UpgradeCredentials,
) -> Result<(TcpStream, Option<WifiSession>), ProtocolError> {
    let address = upgraded_socket(credentials).ok_or_else(|| {
        ProtocolError::Io(io::Error::new(
            io::ErrorKind::NotFound,
            "upgrade route is missing",
        ))
    })?;
    let Some(manager) = manager else {
        return Ok((connect_lan(address)?, None));
    };
    let mut network_credentials = HotspotCredentials::new(
        credentials.ssid.clone().unwrap_or_default(),
        credentials.password.clone().unwrap_or_default(),
    )
    .with_port(address.port());
    if let Some(frequency) = credentials
        .frequency
        .and_then(|frequency| u32::try_from(frequency).ok())
    {
        network_credentials = network_credentials.with_frequency(frequency);
    }
    if let Some(gateway) = credentials.gateway {
        network_credentials = network_credentials.with_gateway(gateway);
    }
    let session = match medium {
        Medium::WifiHotspot => manager
            .join_hotspot(&network_credentials, UPGRADE_DEADLINE)
            .map_err(|error| {
                ProtocolError::Io(io::Error::other(error.to_string()))
            })?,
        Medium::WifiDirect => {
            let discovery = manager
                .find_wifi_direct_peers(UPGRADE_DEADLINE)
                .map_err(|error| {
                    ProtocolError::Io(io::Error::other(error.to_string()))
                })?;
            let joined = (|| {
                let peer = discovery
                    .next_peer(UPGRADE_DEADLINE)
                    .map_err(|error| {
                        ProtocolError::Io(io::Error::other(error.to_string()))
                    })?
                    .ok_or_else(|| {
                        ProtocolError::Io(io::Error::new(
                            io::ErrorKind::NotFound,
                            "Wi-Fi Direct owner was not discovered",
                        ))
                    })?;
                manager
                    .join_wifi_direct(
                        &peer,
                        &network_credentials,
                        UPGRADE_DEADLINE,
                    )
                    .map_err(|error| {
                        ProtocolError::Io(io::Error::other(error.to_string()))
                    })
            })();
            let _stopped = discovery.stop();
            joined?
        }
        Medium::WifiLan => return Ok((connect_lan(address)?, None)),
        Medium::Ble | Medium::Bluetooth => {
            return Err(ProtocolError::Io(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                "upgrade medium failed",
            )));
        }
    };
    let stream = connect_lan(address)?;
    Ok((stream, Some(session)))
}

fn upgraded_socket(credentials: &UpgradeCredentials) -> Option<SocketAddrV4> {
    let ip = credentials.ip_address.or(credentials.gateway)?;
    let port = credentials.port?;
    Some(SocketAddrV4::new(ip, port))
}

fn wait_for_upgrade_stream(listener: &TcpListener) -> io::Result<TcpStream> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now()
        .checked_add(UPGRADE_DEADLINE)
        .unwrap_or_else(Instant::now);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "upgrade accept timed out",
                    ));
                }
                thread::sleep(UPGRADE_ACCEPT_POLL);
            }
            Err(error) => return Err(error),
        }
    }
}

trait MediumRank {
    fn rank(self) -> u8;
}

impl MediumRank for Medium {
    fn rank(self) -> u8 {
        match self {
            Self::Ble => 1,
            Self::Bluetooth => 2,
            Self::WifiLan => 3,
            Self::WifiHotspot | Self::WifiDirect => 4,
        }
    }
}
