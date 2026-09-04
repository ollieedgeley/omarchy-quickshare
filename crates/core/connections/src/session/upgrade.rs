use quickshare_wire::connections::{
    BandwidthUpgradeNegotiationFrame,
    bandwidth_upgrade_negotiation_frame::{
        EventType as WireEventType, UpgradePathInfo,
        upgrade_path_info::{
            Medium as WireMedium, WifiDirectCredentials,
            WifiHotspotCredentials, WifiLanSocket,
        },
    },
};

/// A Nearby Connections medium known to this endpoint.
#[expect(
    clippy::exhaustive_enums,
    reason = "Nearby media known to this endpoint are a closed local set"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Medium {
    /// Bluetooth Low Energy.
    Ble,
    /// Bluetooth Classic.
    Bluetooth,
    /// Same-LAN TCP.
    WifiLan,
    /// Wi-Fi hotspot.
    WifiHotspot,
    /// Wi-Fi Direct.
    WifiDirect,
}

/// The negotiated move or fallback for an active connection.
#[expect(
    clippy::exhaustive_enums,
    reason = "Stay, upgrade, and fallback are the complete local decision set"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpgradeDecision {
    /// Keep the current medium.
    Stay,
    /// Move the connection to a higher-bandwidth medium.
    Upgrade(Medium),
    /// Return to the medium that still carries the payload.
    Fallback(Medium),
}

/// Local bandwidth-upgrade negotiation state.
#[expect(
    clippy::exhaustive_enums,
    reason = "Idle, offered, accepted, and failed are the complete local states"
)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpgradeState {
    /// No upgrade is in flight.
    Idle,
    /// A higher-bandwidth medium has been offered.
    Offered(Medium),
    /// The offered medium was accepted and is replacing the current path.
    Accepted(Medium),
    /// The offered medium failed and payload continues on `fallback`.
    Failed {
        /// Medium that could not carry the connection.
        attempted: Medium,
        /// Medium that still owns the payload.
        fallback: Medium,
    },
}

/// Credentials and addressing carried by an upgrade-path frame.
#[expect(
    clippy::exhaustive_structs,
    reason = "Upgrade credentials are a closed local wire encoding"
)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpgradeCredentials {
    /// Optional operating frequency in MHz.
    pub frequency: Option<i32>,
    /// Advertised IPv4 gateway.
    pub gateway: Option<core::net::Ipv4Addr>,
    /// Advertised IPv4 listen address.
    pub ip_address: Option<core::net::Ipv4Addr>,
    /// Hotspot or Wi-Fi Direct passphrase.
    pub password: Option<String>,
    /// TCP port that accepts the upgraded connection.
    pub port: Option<u16>,
    /// Hotspot or Wi-Fi Direct SSID.
    pub ssid: Option<String>,
    /// Wi-Fi Direct device name used during association.
    pub device_name: Option<String>,
    /// Wi-Fi Direct PIN, when the peer requires WPS.
    pub pin: Option<String>,
}

/// A bandwidth-upgrade frame observed on the encrypted connection.
#[expect(
    clippy::exhaustive_enums,
    reason = "Upgrade events are the complete local Nearby frame set"
)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpgradeEvent {
    /// The peer asked this endpoint to advertise an upgrade path.
    PathRequest {
        /// Media the requester can join.
        mediums: Vec<Medium>,
    },
    /// The peer advertised an upgrade path.
    PathAvailable {
        /// Offered medium.
        medium: Medium,
        /// Medium-specific credentials and routes.
        credentials: UpgradeCredentials,
    },
    /// The peer finished writing on the prior channel.
    LastWriteToPriorChannel,
    /// The peer finished reading the prior channel and it may close.
    SafeToClosePriorChannel {
        /// Optional station frequency in MHz.
        sta_frequency: Option<i32>,
    },
    /// The joiner identified itself on the new channel.
    ClientIntroduction {
        /// Connections endpoint id carried on the new channel.
        endpoint_id: String,
    },
    /// The host acknowledged the joiner on the new channel.
    ClientIntroductionAck,
    /// The peer reported that the offered path failed.
    Failure {
        /// Medium that failed.
        medium: Medium,
    },
}

impl Medium {
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "Unsupported Nearby media stay unmapped rather than guessed"
    )]
    pub(super) fn from_wire(value: i32) -> Option<Self> {
        use WireMedium as Wire;
        match Wire::try_from(value).ok()? {
            Wire::Ble => Some(Self::Ble),
            Wire::Bluetooth => Some(Self::Bluetooth),
            Wire::WifiLan => Some(Self::WifiLan),
            Wire::WifiHotspot => Some(Self::WifiHotspot),
            Wire::WifiDirect => Some(Self::WifiDirect),
            _ => None,
        }
    }

    pub(super) const fn rank(self) -> u8 {
        match self {
            Self::Ble => 1,
            Self::Bluetooth => 2,
            Self::WifiLan => 3,
            Self::WifiHotspot | Self::WifiDirect => 4,
        }
    }

    pub(super) const fn to_wire(self) -> i32 {
        use WireMedium as Wire;
        match self {
            Self::Ble => Wire::Ble as i32,
            Self::Bluetooth => Wire::Bluetooth as i32,
            Self::WifiLan => Wire::WifiLan as i32,
            Self::WifiHotspot => Wire::WifiHotspot as i32,
            Self::WifiDirect => Wire::WifiDirect as i32,
        }
    }
}

impl UpgradeDecision {
    /// Chooses upgrade, stay, or no implicit downgrade from two media.
    #[must_use]
    pub const fn from_media(current: Medium, offered: Medium) -> Self {
        if offered.rank() > current.rank() {
            Self::Upgrade(offered)
        } else {
            Self::Stay
        }
    }
}

impl UpgradeCredentials {
    pub(super) fn from_path_info(info: &UpgradePathInfo) -> Self {
        if let Some(hotspot) = info.wifi_hotspot_credentials.as_ref() {
            return Self::from_hotspot(hotspot);
        }
        if let Some(direct) = info.wifi_direct_credentials.as_ref() {
            return Self::from_direct(direct);
        }
        if let Some(lan) = info.wifi_lan_socket.as_ref() {
            return Self::from_lan(lan);
        }
        Self::default()
    }

    fn from_direct(credentials: &WifiDirectCredentials) -> Self {
        let gateway = credentials
            .gateway
            .as_deref()
            .and_then(|value| value.parse().ok());
        Self {
            frequency: credentials.frequency,
            gateway,
            ip_address: gateway,
            password: credentials.password.clone(),
            port: credentials.port.and_then(|port| u16::try_from(port).ok()),
            ssid: credentials.ssid.clone(),
            device_name: credentials.device_name.clone(),
            pin: credentials.pin.clone(),
        }
    }

    #[expect(
        clippy::return_and_then,
        reason = "optional port conversion stays a closed Option chain"
    )]
    fn from_hotspot(credentials: &WifiHotspotCredentials) -> Self {
        let gateway = credentials
            .gateway
            .as_deref()
            .and_then(|value| value.parse().ok());
        let candidate = credentials
            .address_candidates
            .iter()
            .find_map(|entry| ipv4_addr(entry.ip_address.as_deref()));
        let port = credentials
            .address_candidates
            .iter()
            .find_map(|entry| {
                entry.port.and_then(|port| u16::try_from(port).ok())
            })
            .or_else(|| {
                credentials.port.and_then(|port| u16::try_from(port).ok())
            });
        Self {
            frequency: credentials.frequency,
            gateway,
            ip_address: candidate,
            password: credentials.password.clone(),
            port,
            ssid: credentials.ssid.clone(),
            device_name: None,
            pin: None,
        }
    }
    #[expect(
        clippy::return_and_then,
        reason = "optional port conversion stays a closed Option chain"
    )]
    fn from_lan(socket: &WifiLanSocket) -> Self {
        let candidate = socket
            .address_candidates
            .iter()
            .find_map(|entry| ipv4_addr(entry.ip_address.as_deref()));
        let ip_address =
            candidate.or_else(|| ipv4_addr(socket.ip_address.as_deref()));
        let port = socket
            .address_candidates
            .iter()
            .find_map(|entry| {
                entry.port.and_then(|port| u16::try_from(port).ok())
            })
            .or_else(|| {
                socket.wifi_port.and_then(|port| u16::try_from(port).ok())
            });
        Self {
            frequency: None,
            gateway: None,
            ip_address,
            password: None,
            port,
            ssid: None,
            device_name: None,
            pin: None,
        }
    }

    #[expect(
        clippy::wrong_self_convention,
        reason = "into_path_info encodes a wire frame from borrowed fields"
    )]
    pub(super) fn into_path_info(&self, medium: Medium) -> UpgradePathInfo {
        let tcp_ip = self.gateway.or(self.ip_address);
        let candidates = self.address_candidates();
        let mut info = UpgradePathInfo {
            medium: Some(medium.to_wire()),
            supports_client_introduction_ack: Some(true),
            ..Default::default()
        };
        match medium {
            Medium::WifiHotspot => {
                info.wifi_hotspot_credentials = Some(WifiHotspotCredentials {
                    ssid: self.ssid.clone(),
                    password: self.password.clone(),
                    port: self.port.map(i32::from),
                    gateway: tcp_ip.map(|gateway| gateway.to_string()),
                    frequency: self.frequency,
                    address_candidates: candidates,
                });
            }
            Medium::WifiDirect => {
                info.wifi_direct_credentials = Some(WifiDirectCredentials {
                    ssid: self.ssid.clone(),
                    password: self.password.clone(),
                    port: self.port.map(i32::from),
                    frequency: self.frequency,
                    gateway: tcp_ip.map(|gateway| gateway.to_string()),
                    device_name: self.device_name.clone(),
                    pin: self.pin.clone(),
                    ..Default::default()
                });
            }
            Medium::WifiLan => {
                info.wifi_lan_socket = Some(WifiLanSocket {
                    ip_address: self
                        .ip_address
                        .map(|address| address.octets().to_vec()),
                    wifi_port: self.port.map(i32::from),
                    address_candidates: candidates,
                });
            }
            Medium::Ble | Medium::Bluetooth => {}
        }
        info
    }

    fn address_candidates(
        &self,
    ) -> Vec<quickshare_wire::connections::ServiceAddress> {
        use quickshare_wire::connections::ServiceAddress;
        let port = self.port.map(i32::from);
        let mut addresses = Vec::new();
        if let Some(ip_address) = self.ip_address {
            addresses.push(ServiceAddress {
                ip_address: Some(ip_address.octets().to_vec()),
                port,
            });
        }
        if let Some(gateway) = self.gateway
            && self.ip_address != Some(gateway)
        {
            addresses.push(ServiceAddress {
                ip_address: Some(gateway.octets().to_vec()),
                port,
            });
        }
        addresses
    }
}

fn ipv4_addr(bytes: Option<&[u8]>) -> Option<core::net::Ipv4Addr> {
    let octets: [u8; 4] = bytes?.try_into().ok()?;
    Some(core::net::Ipv4Addr::from(octets))
}

#[expect(
    clippy::multiple_inherent_impl,
    clippy::too_many_lines,
    reason = "Upgrade frames stay with credentials; methods stay split"
)]
impl super::Connection {
    pub(super) fn upgrade_event(
        &mut self,
        negotiation: BandwidthUpgradeNegotiationFrame,
    ) -> Result<Option<super::Event>, super::Error> {
        use super::{Error, Event};
        use WireEventType as EventType;
        let event_type =
            negotiation.event_type.ok_or(Error::UnexpectedFrame)?;
        if event_type == EventType::UpgradePathRequest as i32 {
            let mediums = negotiation
                .upgrade_path_info
                .and_then(|info| info.upgrade_path_request)
                .map(|request| {
                    request
                        .mediums
                        .into_iter()
                        .filter_map(Medium::from_wire)
                        .collect()
                })
                .unwrap_or_default();
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::PathRequest { mediums },
            }));
        }
        if event_type == EventType::UpgradePathAvailable as i32 {
            let info = negotiation
                .upgrade_path_info
                .ok_or(Error::UnexpectedFrame)?;
            let medium =
                Medium::from_wire(info.medium.ok_or(Error::UnexpectedFrame)?)
                    .ok_or(Error::UnexpectedFrame)?;
            let credentials = UpgradeCredentials::from_path_info(&info);
            self.upgrade = UpgradeState::Offered(medium);
            self.upgrade_host = false;
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::PathAvailable {
                    medium,
                    credentials,
                },
            }));
        }
        if event_type == EventType::LastWriteToPriorChannel as i32 {
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::LastWriteToPriorChannel,
            }));
        }
        if event_type == EventType::SafeToClosePriorChannel as i32 {
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::SafeToClosePriorChannel {
                    sta_frequency: negotiation
                        .safe_to_close_prior_channel
                        .and_then(|frame| frame.sta_frequency),
                },
            }));
        }
        if event_type == EventType::ClientIntroduction as i32 {
            let endpoint_id = negotiation
                .client_introduction
                .and_then(|introduction| introduction.endpoint_id)
                .unwrap_or_default();
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::ClientIntroduction { endpoint_id },
            }));
        }
        if event_type == EventType::ClientIntroductionAck as i32 {
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::ClientIntroductionAck,
            }));
        }
        if event_type == EventType::UpgradeFailure as i32 {
            let medium = Medium::from_wire(
                negotiation
                    .upgrade_path_info
                    .and_then(|info| info.medium)
                    .ok_or(Error::UnexpectedFrame)?,
            )
            .ok_or(Error::UnexpectedFrame)?;
            self.upgrade = UpgradeState::Failed {
                attempted: medium,
                fallback: self.medium,
            };
            self.upgrade_host = false;
            return Ok(Some(Event::Upgrade {
                event: UpgradeEvent::Failure { medium },
            }));
        }
        Err(Error::UnexpectedFrame)
    }
}
