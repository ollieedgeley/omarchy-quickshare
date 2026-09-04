//! Discovery waits for BlueZ object-manager and property signals.

use core::pin::pin;
use core::task::Poll;
use std::time::{Duration, Instant};

use futures_lite::StreamExt;
use zbus::MessageStream;
use zbus::blocking::Connection;
use zbus::message::Type;

use crate::radio::Error;

/// Waits until `ready` succeeds or `deadline` elapses.
pub(super) fn wait_until_or_timeout<T, F>(
    connection: &Connection,
    deadline: Instant,
    mut ready: F,
) -> Result<T, Error>
where
    F: FnMut() -> Result<T, Error>,
{
    loop {
        match ready() {
            Ok(value) => return Ok(value),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::timeout());
        }
        wait_for_change(connection, remaining)?;
    }
}

/// Blocks until InterfacesAdded or PropertiesChanged, or `timeout`.
pub(super) fn wait_for_change(
    connection: &Connection,
    timeout: Duration,
) -> Result<(), Error> {
    let inner = connection.inner().clone();
    async_io::block_on(async move {
        let manager = zbus::fdo::ObjectManagerProxy::builder(&inner)
            .destination("org.bluez")
            .map_err(|error| Error::bus(error.to_string()))?
            .path("/")
            .map_err(|error| Error::bus(error.to_string()))?
            .build()
            .await
            .map_err(|error| Error::bus(error.to_string()))?;
        let added = manager
            .receive_interfaces_added()
            .await
            .map_err(|error| Error::bus(error.to_string()))?;
        let rule = zbus::MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .map_err(|error| Error::bus(error.to_string()))?
            .member("PropertiesChanged")
            .map_err(|error| Error::bus(error.to_string()))?
            .build();
        let props = MessageStream::for_match_rule(rule, &inner, Some(8))
            .await
            .map_err(|error| Error::bus(error.to_string()))?;
        race_change(added, props, timeout).await
    })
}

async fn race_change<A, P>(
    mut added: A,
    mut props: P,
    timeout: Duration,
) -> Result<(), Error>
where
    A: futures_lite::Stream + Unpin,
    P: futures_lite::Stream + Unpin,
{
    let mut added_next = pin!(added.next());
    let mut props_next = pin!(props.next());
    let mut timer = pin!(async_io::Timer::after(timeout));
    core::future::poll_fn(|context| {
        if added_next.as_mut().poll(context).is_ready() {
            return Poll::Ready(Ok(()));
        }
        if props_next.as_mut().poll(context).is_ready() {
            return Poll::Ready(Ok(()));
        }
        if timer.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err(Error::timeout()));
        }
        Poll::Pending
    })
    .await
}
