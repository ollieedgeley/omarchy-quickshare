//! NetworkManager D-Bus adapter for hotspot and Wi-Fi Direct roles.

mod bus;

use core::fmt;
use core::net::Ipv4Addr;
use core::time::Duration;

use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

use self::bus::nm_error;
use crate::Error;

/// A high-bandwidth medium exposed by the adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Medium {
    /// An existing local IP network.
    Lan,
    /// A temporary Wi-Fi hotspot.
    Hotspot,
    /// A Wi-Fi Direct group.
    WifiDirect,
}

/// Local role on a medium.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Role {
    /// Join a remote owner or access point.
    Client,
    /// Own the group or access point.
    Owner,
}

/// Established local addressing for one medium.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Candidate {
    pub(super) addresses: Vec<Ipv4Addr>,
    pub(super) gateway: Option<Ipv4Addr>,
    pub(super) medium: Medium,
    pub(super) port: Option<u16>,
    pub(super) role: Role,
}

/// SSID and passphrase used to join or own a Wi-Fi medium.
pub struct Credentials {
    pub(super) frequency: Option<u32>,
    pub(super) gateway: Option<Ipv4Addr>,
    pub(super) password: String,
    pub(super) port: Option<u16>,
    pub(super) ssid: String,
}

/// A discovered Wi-Fi Direct peer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Peer {
    address: String,
    name: String,
}

/// A live NetworkManager D-Bus connection.
#[derive(Clone)]
pub struct NetworkManager {
    pub(super) connection: Connection,
}

/// An established hotspot or Wi-Fi Direct session.
pub struct Session {
    pub(super) active: Option<OwnedObjectPath>,
    pub(super) candidate: Candidate,
    pub(super) manager: NetworkManager,
    pub(super) profile: Option<OwnedObjectPath>,
}

/// An active Wi-Fi Direct find operation.
pub struct Discovery {
    pub(super) device: OwnedObjectPath,
    pub(super) manager: NetworkManager,
    pub(super) stopped: bool,
}

impl Credentials {
    /// Builds credentials from an SSID and passphrase.
    #[must_use]
    #[inline]
    pub const fn new(ssid: String, password: String) -> Self {
        Self {
            frequency: None,
            gateway: None,
            password,
            port: None,
            ssid,
        }
    }

    /// Returns the SSID.
    #[must_use]
    #[inline]
    pub fn ssid(&self) -> &str {
        &self.ssid
    }

    /// Returns the passphrase.
    #[must_use]
    #[inline]
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Returns the optional operating frequency in MHz.
    #[must_use]
    #[inline]
    pub const fn frequency(&self) -> Option<u32> {
        self.frequency
    }

    /// Returns the optional advertised gateway.
    #[must_use]
    #[inline]
    pub const fn gateway(&self) -> Option<Ipv4Addr> {
        self.gateway
    }

    /// Returns the optional TCP port advertised with the medium.
    #[must_use]
    #[inline]
    pub const fn port(&self) -> Option<u16> {
        self.port
    }

    /// Sets the operating frequency in MHz.
    #[must_use]
    #[inline]
    pub const fn with_frequency(mut self, frequency: u32) -> Self {
        self.frequency = Some(frequency);
        self
    }

    /// Sets the advertised gateway address.
    #[must_use]
    #[inline]
    pub const fn with_gateway(mut self, gateway: Ipv4Addr) -> Self {
        self.gateway = Some(gateway);
        self
    }

    /// Sets the TCP port carried with the medium credentials.
    #[must_use]
    #[inline]
    pub const fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }
}

impl Candidate {
    /// Returns assigned local IPv4 addresses.
    #[must_use]
    #[inline]
    pub fn addresses(&self) -> &[Ipv4Addr] {
        &self.addresses
    }

    /// Returns the gateway advertised or assigned for this session.
    #[must_use]
    #[inline]
    pub const fn gateway(&self) -> Option<Ipv4Addr> {
        self.gateway
    }

    /// Returns the medium this session occupies.
    #[must_use]
    #[inline]
    pub const fn medium(&self) -> Medium {
        self.medium
    }

    /// Returns the TCP port advertised with the medium, if any.
    #[must_use]
    #[inline]
    pub const fn port(&self) -> Option<u16> {
        self.port
    }

    /// Returns the local role on the medium.
    #[must_use]
    #[inline]
    pub const fn role(&self) -> Role {
        self.role
    }
}

impl Peer {
    pub(super) fn new(address: String, name: String) -> Self {
        Self { address, name }
    }

    /// Returns the peer P2P device address.
    #[must_use]
    #[inline]
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Returns the peer display name.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl NetworkManager {
    /// Connects to the system NetworkManager bus.
    ///
    /// # Errors
    ///
    /// Returns an error when the system bus is unavailable.
    #[inline]
    pub fn system() -> Result<Self, Error> {
        Connection::system()
            .map(Self::from_connection)
            .map_err(nm_error)
    }

    /// Connects to NetworkManager on a D-Bus address.
    ///
    /// # Errors
    ///
    /// Returns an error when the bus cannot be reached.
    #[inline]
    pub fn at(address: &str) -> Result<Self, Error> {
        zbus::blocking::connection::Builder::address(address)
            .map_err(nm_error)?
            .build()
            .map(Self::from_connection)
            .map_err(nm_error)
    }

    /// Joins a hotspot as a station.
    ///
    /// # Errors
    ///
    /// Returns an error when activation fails or the timeout elapses.
    #[inline]
    pub fn join_hotspot(
        &self,
        credentials: &Credentials,
        timeout: Duration,
    ) -> Result<Session, Error> {
        self.activate_hotspot(credentials, timeout, Role::Client)
    }

    /// Starts a hotspot as the access-point owner.
    ///
    /// # Errors
    ///
    /// Returns an error when activation fails or the timeout elapses.
    #[inline]
    pub fn start_hotspot(
        &self,
        credentials: &Credentials,
        timeout: Duration,
    ) -> Result<Session, Error> {
        self.activate_hotspot(credentials, timeout, Role::Owner)
    }

    /// Starts Wi-Fi Direct peer discovery.
    ///
    /// # Errors
    ///
    /// Returns an error when no P2P device exists or `StartFind` fails.
    #[inline]
    pub fn find_wifi_direct_peers(
        &self,
        timeout: Duration,
    ) -> Result<Discovery, Error> {
        self.start_find(timeout)
    }

    /// Joins a Wi-Fi Direct group as a client.
    ///
    /// # Errors
    ///
    /// Returns an error when activation fails or the timeout elapses.
    #[inline]
    pub fn join_wifi_direct(
        &self,
        peer: &Peer,
        credentials: &Credentials,
        timeout: Duration,
    ) -> Result<Session, Error> {
        self.activate_wifi_direct(Some(peer), credentials, timeout)
    }

    /// Starts a Wi-Fi Direct group as owner.
    ///
    /// # Errors
    ///
    /// Returns an error when activation fails or the timeout elapses.
    #[inline]
    pub fn start_wifi_direct(
        &self,
        credentials: &Credentials,
        timeout: Duration,
    ) -> Result<Session, Error> {
        self.activate_wifi_direct(None, credentials, timeout)
    }

    pub(super) const fn from_connection(connection: Connection) -> Self {
        Self { connection }
    }
}

impl Discovery {
    /// Waits for one discovered peer.
    ///
    /// # Errors
    ///
    /// Returns an error when the P2P device disappears.
    #[inline]
    pub fn next_peer(&self, timeout: Duration) -> Result<Option<Peer>, Error> {
        self.wait_peer(timeout)
    }

    /// Stops peer discovery.
    ///
    /// # Errors
    ///
    /// Returns an error when `StopFind` fails.
    #[inline]
    pub fn stop(mut self) -> Result<(), Error> {
        self.stop_find()
    }
}

impl Session {
    /// Returns addressing and role for this session.
    #[must_use]
    #[inline]
    pub const fn candidate(&self) -> &Candidate {
        &self.candidate
    }

    /// Deactivates the session and deletes the temporary profile.
    ///
    /// # Errors
    ///
    /// Returns an error when NetworkManager rejects cleanup.
    #[inline]
    pub fn disconnect(mut self) -> Result<(), Error> {
        self.teardown()
    }
}

impl fmt::Debug for Credentials {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("ssid", &self.ssid)
            .field("password", &"<redacted>")
            .field("frequency", &self.frequency)
            .field("gateway", &self.gateway)
            .field("port", &self.port)
            .finish()
    }
}

impl fmt::Debug for NetworkManager {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkManager").finish_non_exhaustive()
    }
}

impl fmt::Debug for Session {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("candidate", &self.candidate)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for Discovery {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Discovery")
            .field("device", &self.device)
            .finish_non_exhaustive()
    }
}
