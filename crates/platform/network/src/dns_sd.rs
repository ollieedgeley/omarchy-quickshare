use alloc::collections::BTreeMap;
use core::fmt;
use core::net::{IpAddr, Ipv4Addr};
use core::time::Duration;
use std::time::Instant;

use mdns_sd::{
    Receiver, RecvTimeoutError, ResolvedService as MdnsResolvedService,
    ServiceDaemon, ServiceEvent, ServiceInfo,
};

use crate::Error;

/// One DNS-SD service to announce on the local network.
#[derive(Clone, Debug, Eq, PartialEq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "Callers construct DNS-SD advertisements with the peer record"
)]
pub struct Advertisement {
    /// IPv4 addresses that accept the advertised TCP connection.
    pub addresses: Vec<Ipv4Addr>,
    /// Local DNS hostname ending in `.local.`.
    pub hostname: String,
    /// Service instance label.
    pub instance: String,
    /// Listening TCP port.
    pub port: u16,
    /// DNS-SD TXT properties.
    pub properties: BTreeMap<String, String>,
    /// Complete DNS-SD service type.
    pub service_type: String,
}

/// A resolved DNS-SD service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedService {
    /// IPv4 addresses advertised by the peer.
    addresses: Vec<Ipv4Addr>,
    /// DNS-SD instance label advertised by the peer.
    instance: String,
    /// TCP port advertised by the peer.
    port: u16,
    /// DNS-SD TXT properties advertised by the peer.
    properties: BTreeMap<String, String>,
}

/// One active DNS-SD registration.
#[derive(Clone)]
pub struct Registration {
    /// Daemon that owns this registration.
    daemon: ServiceDaemon,
    /// Fully qualified DNS-SD service name.
    fullname: String,
}

/// One active DNS-SD browser.
pub struct Browser {
    /// Daemon that owns this browse operation.
    daemon: ServiceDaemon,
    /// Events emitted by the daemon for this browse operation.
    receiver: Receiver<ServiceEvent>,
    /// Complete service type passed to the daemon.
    service_type: String,
}

/// The shared in-process DNS-SD service.
#[derive(Clone)]
pub struct DnsSd {
    /// Shared daemon used for registrations and browsing.
    daemon: ServiceDaemon,
}

impl Browser {
    /// Waits for one resolved service.
    ///
    /// # Errors
    ///
    /// Returns an error when the discovery channel disconnects.
    #[inline]
    pub fn resolve(
        &self,
        timeout: Duration,
    ) -> Result<Option<ResolvedService>, Error> {
        let started = Instant::now();
        loop {
            let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
                return Ok(None);
            };
            match self.receiver.recv_timeout(remaining) {
                Ok(ServiceEvent::ServiceResolved(info)) => {
                    return Ok(Some(resolve_service(
                        &info,
                        &self.service_type,
                    )));
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(Error(String::from(
                        "mDNS discovery channel disconnected",
                    )));
                }
            }
        }
    }

    /// Stops this service browse.
    ///
    /// # Errors
    ///
    /// Returns an error when the DNS-SD daemon rejects the request.
    #[inline]
    pub fn stop(self) -> Result<(), Error> {
        self.daemon
            .stop_browse(&self.service_type)
            .map_err(convert_error)
    }
}

impl DnsSd {
    /// Announces one TCP service.
    ///
    /// # Errors
    ///
    /// Returns an error when the advertisement is invalid or cannot be sent.
    #[inline]
    pub fn advertise(
        &self,
        advertisement: &Advertisement,
    ) -> Result<Registration, Error> {
        let properties = advertisement.properties.iter().collect::<Vec<_>>();
        let addresses = advertisement
            .addresses
            .iter()
            .copied()
            .map(IpAddr::V4)
            .collect::<Vec<_>>();
        let service = ServiceInfo::new(
            &advertisement.service_type,
            &advertisement.instance,
            &advertisement.hostname,
            addresses.as_slice(),
            advertisement.port,
            properties.as_slice(),
        )
        .map_err(convert_error)?;
        let fullname = service.get_fullname().to_owned();
        self.daemon.register(service).map_err(convert_error)?;
        Ok(Registration {
            daemon: self.daemon.clone(),
            fullname,
        })
    }

    /// Starts browsing one complete DNS-SD service type.
    ///
    /// # Errors
    ///
    /// Returns an error when the service type is invalid or browsing fails.
    #[inline]
    pub fn browse(&self, service_type: &str) -> Result<Browser, Error> {
        self.daemon
            .browse(service_type)
            .map(|receiver| Browser {
                daemon: self.daemon.clone(),
                receiver,
                service_type: service_type.to_owned(),
            })
            .map_err(convert_error)
    }

    /// Starts an in-process DNS-SD daemon.
    ///
    /// # Errors
    ///
    /// Returns an error when multicast sockets cannot be created.
    #[inline]
    pub fn new() -> Result<Self, Error> {
        ServiceDaemon::new()
            .map(|daemon| Self { daemon })
            .map_err(convert_error)
    }

    /// Stops the underlying DNS-SD worker.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker cannot receive the shutdown request.
    #[inline]
    pub fn shutdown(self) -> Result<(), Error> {
        self.daemon
            .shutdown()
            .map(|_status| ())
            .map_err(convert_error)
    }
}

impl Registration {
    /// Gracefully removes this service advertisement.
    ///
    /// # Errors
    ///
    /// Returns an error when the DNS-SD daemon rejects the request.
    #[inline]
    pub fn stop(self) -> Result<(), Error> {
        self.daemon
            .unregister(&self.fullname)
            .map(|_status| ())
            .map_err(convert_error)
    }
}

impl ResolvedService {
    /// Returns every advertised IPv4 address.
    #[must_use]
    #[inline]
    pub fn addresses(&self) -> &[Ipv4Addr] {
        &self.addresses
    }

    /// Returns the service instance label.
    #[must_use]
    #[inline]
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// Returns the advertised TCP port.
    #[must_use]
    #[inline]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Returns one DNS-SD TXT value.
    #[must_use]
    #[inline]
    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(String::as_str)
    }
}

impl fmt::Debug for Browser {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Browser")
            .field("service_type", &self.service_type)
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for DnsSd {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DnsSd").finish_non_exhaustive()
    }
}

impl fmt::Debug for Registration {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registration")
            .field("fullname", &self.fullname)
            .finish_non_exhaustive()
    }
}

/// Returns every usable local IPv4 address for a LAN advertisement.
///
/// # Errors
///
/// Returns an error when the operating system cannot enumerate interfaces or
/// when no non-loopback IPv4 address exists.
#[inline]
pub fn local_ipv4_addresses() -> Result<Vec<Ipv4Addr>, Error> {
    let interfaces =
        if_addrs::get_if_addrs().map_err(|error| Error(error.to_string()))?;
    let mut addresses = interfaces
        .into_iter()
        .filter_map(|interface| match interface.addr {
            if_addrs::IfAddr::V4(address)
                if !address.ip.is_loopback()
                    && !address.ip.is_unspecified() =>
            {
                Some(address.ip)
            }
            if_addrs::IfAddr::V4(_) | if_addrs::IfAddr::V6(_) => None,
        })
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    if addresses.is_empty() {
        return Err(Error(String::from(
            "no non-loopback local IPv4 address is available",
        )));
    }
    Ok(addresses)
}

fn convert_error(error: mdns_sd::Error) -> Error {
    Error(error.to_string())
}

#[expect(
    clippy::single_call_fn,
    reason = "The named conversion separates daemon data from the public result"
)]
fn resolve_service(
    info: &MdnsResolvedService,
    service_type: &str,
) -> ResolvedService {
    let instance = info
        .get_fullname()
        .strip_suffix(service_type)
        .unwrap_or_else(|| info.get_fullname())
        .trim_end_matches('.')
        .to_owned();
    let addresses = info.get_addresses_v4().into_iter().collect();
    let properties = info
        .get_properties()
        .iter()
        .map(|property| {
            (property.key().to_owned(), property.val_str().to_owned())
        })
        .collect();
    ResolvedService {
        addresses,
        instance,
        port: info.get_port(),
        properties,
    }
}
