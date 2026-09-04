//! Connectable Quick Share BLE receiver advertisements.

use crate::QUICK_SHARE_BLE_UUID;
use crate::radio::{AdapterSlot, Error, FakeSession, lock, next_lease};

/// Bytes advertised under the Quick Share BLE service.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiverAdvertisement {
    /// Service data payload.
    service_data: Vec<u8>,
}

/// An active BLE advertisement lease.
#[derive(Debug)]
pub struct BleAdvertisement {
    /// Fake or D-Bus cleanup action.
    inner: AdvertisementInner,
}

/// Backend-specific advertisement state.
#[derive(Debug)]
pub(crate) enum AdvertisementInner {
    /// Production registration against BlueZ.
    Dbus(crate::bus::DbusAdvertisement),
    /// In-process lease.
    Fake {
        /// Owning adapter session.
        session: FakeSession,
        /// Lease identifier.
        lease: u64,
    },
}

impl ReceiverAdvertisement {
    /// Creates a receiver advertisement from service data.
    #[must_use]
    #[inline]
    pub fn new(service_data: Vec<u8>) -> Self {
        Self { service_data }
    }

    /// Returns the advertised service data.
    #[must_use]
    #[inline]
    pub fn service_data(&self) -> &[u8] {
        &self.service_data
    }
}

impl BleAdvertisement {
    /// Builds a fake advertisement lease.
    #[must_use]
    #[inline]
    pub(crate) fn fake(session: FakeSession, lease: u64) -> Self {
        Self {
            inner: AdvertisementInner::Fake { session, lease },
        }
    }

    /// Builds a production advertisement lease.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusAdvertisement) -> Self {
        Self {
            inner: AdvertisementInner::Dbus(handle),
        }
    }

    /// Unregisters the advertisement.
    ///
    /// # Errors
    ///
    /// Returns an error when BlueZ rejects unregistration.
    #[inline]
    pub fn stop(mut self) -> Result<(), Error> {
        self.unregister()
    }

    /// Releases the lease once.
    fn unregister(&mut self) -> Result<(), Error> {
        match &mut self.inner {
            AdvertisementInner::Dbus(handle) => handle.stop(),
            AdvertisementInner::Fake { session, lease } => {
                let mut radio = lock(&session.radio)?;
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _removed = slot.advertisements.remove(lease);
                }
                Ok(())
            }
        }
    }
}

impl Drop for BleAdvertisement {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

impl FakeSession {
    /// Registers a receiver advertisement on the fake radio.
    pub(crate) fn advertise_receiver(
        &self,
        advertisement: ReceiverAdvertisement,
    ) -> Result<BleAdvertisement, Error> {
        let mut radio = lock(&self.radio)?;
        let lease = next_lease(&mut radio.next_lease);
        let slot = powered_slot(&mut radio.adapters, self.address)?;
        let _previous = slot.advertisements.insert(lease, advertisement);
        Ok(BleAdvertisement::fake(self.clone(), lease))
    }
}

/// Returns a powered adapter slot.
pub(crate) fn powered_slot(
    adapters: &mut alloc::collections::BTreeMap<
        crate::radio::Address,
        AdapterSlot,
    >,
    address: crate::radio::Address,
) -> Result<&mut AdapterSlot, Error> {
    let slot = adapters
        .get_mut(&address)
        .ok_or_else(|| Error::unavailable("adapter is not on the radio"))?;
    if slot.powered {
        Ok(slot)
    } else {
        Err(Error::unavailable("adapter is not powered"))
    }
}

/// Returns the Quick Share BLE UUID used in BlueZ filters.
#[must_use]
#[inline]
pub(crate) const fn service_uuid() -> &'static str {
    QUICK_SHARE_BLE_UUID
}
