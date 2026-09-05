use core::net::{Ipv4Addr, SocketAddrV4};
use std::{io, net::TcpListener, net::TcpStream};

use crate::{Advertisement, DnsSd, Registration};

/// A nonblocking TCP listener owned by the Linux LAN adapter.
#[derive(Debug)]
pub struct Listener {
    /// Bound operating-system listener.
    listener: TcpListener,
    /// Cached bound port assigned during construction.
    port: u16,
}

impl Listener {
    /// Accepts one pending TCP connection without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system rejects the accept request.
    #[inline]
    pub fn accept(&self) -> io::Result<Option<TcpStream>> {
        match self.listener.accept() {
            Ok((stream, _address)) => {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "lan",
                    operation = "accept",
                    outcome = "success"
                );
                Ok(Some(stream))
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(error) => {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "lan",
                    operation = "accept",
                    outcome = "failure",
                    io_error_kind = ?error.kind()
                );
                Err(error)
            }
        }
    }

    /// Binds an ephemeral port on every local IPv4 interface.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket cannot bind or become nonblocking.
    #[inline]
    pub fn bind_any() -> io::Result<Self> {
        Self::bind(0)
    }

    /// Binds the requested port on every local IPv4 interface.
    ///
    /// # Errors
    ///
    /// Returns an error when the socket cannot bind or become nonblocking.
    #[inline]
    pub fn bind(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
            .inspect_err(|error| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "lan",
                    operation = "bind",
                    outcome = "failure",
                    io_error_kind = ?error.kind()
                );
            })?;
        listener.set_nonblocking(true).inspect_err(|error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "lan",
                operation = "set_nonblocking",
                outcome = "failure",
                io_error_kind = ?error.kind()
            );
        })?;
        let bound_port = listener
            .local_addr()
            .inspect_err(|error| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "lan",
                    operation = "read_local_address",
                    outcome = "failure",
                    io_error_kind = ?error.kind()
                );
            })?
            .port();
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "lan",
            operation = "bind",
            outcome = "success"
        );
        Ok(Self {
            listener,
            port: bound_port,
        })
    }

    /// Returns the bound local TCP port.
    #[must_use]
    #[inline]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// Publishes this bound listener through DNS-SD.
    ///
    /// # Errors
    ///
    /// Returns an error when the advertised port differs or registration fails.
    #[inline]
    pub fn publish(
        self,
        dns_sd: &DnsSd,
        advertisement: &Advertisement,
    ) -> io::Result<PublishedLanListener> {
        if advertisement.port != self.port {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "dns_sd",
                operation = "publish",
                outcome = "failure",
                reason = "port_mismatch"
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "advertisement port does not match the bound listener",
            ));
        }
        let registration =
            dns_sd.advertise(advertisement).map_err(io::Error::other)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "dns_sd",
            operation = "publish",
            outcome = "success"
        );
        Ok(PublishedLanListener {
            listener: self,
            registration: Some(registration),
        })
    }
}

/// A TCP listener with an active DNS-SD registration.
#[derive(Debug)]
pub struct PublishedLanListener {
    /// Bound nonblocking TCP listener.
    listener: Listener,
    /// Registration withdrawn on explicit stop or drop.
    registration: Option<Registration>,
}

impl PublishedLanListener {
    /// Accepts one pending TCP connection without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system rejects the accept request.
    #[inline]
    pub fn accept(&self) -> io::Result<Option<TcpStream>> {
        self.listener.accept()
    }

    /// Returns the bound local TCP port.
    #[must_use]
    #[inline]
    pub const fn port(&self) -> u16 {
        self.listener.port()
    }

    /// Withdraws the DNS-SD record and closes the listener.
    ///
    /// # Errors
    ///
    /// Returns an error when the DNS-SD daemon rejects unregistration.
    #[inline]
    pub fn stop(mut self) -> io::Result<()> {
        self.unregister()
    }

    /// Withdraws the registration at most once.
    fn unregister(&mut self) -> io::Result<()> {
        let result = self.registration.take().map_or(Ok(()), |registration| {
            registration.stop().map_err(io::Error::other)
        });
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "dns_sd",
            operation = "unregister",
            outcome = if result.is_ok() { "requested" } else { "failure" }
        );
        result
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for PublishedLanListener {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

/// Connects to one resolved LAN route.
///
/// # Errors
///
/// Returns an error when the TCP connection cannot be established.
#[inline]
pub fn connect(route: SocketAddrV4) -> io::Result<TcpStream> {
    TcpStream::connect(route)
        .inspect(|_| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "lan",
                operation = "connect",
                outcome = "success"
            );
        })
        .inspect_err(|error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "lan",
                operation = "connect",
                outcome = "failure",
                io_error_kind = ?error.kind()
            );
        })
}
