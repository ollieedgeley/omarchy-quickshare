//! BLE scan leases and candidate polling.

use alloc::collections::BTreeSet;
use core::time::Duration;

use crate::advertisement::powered_slot;
use crate::radio::{
    AdapterSlot, Address, BleCandidate, Error, FakeSession, RadioInner, lock,
    next_lease,
};

/// An active BLE discovery lease.
#[derive(Debug)]
pub struct BleScan {
    /// Fake or D-Bus scan state.
    inner: ScanInner,
    /// Addresses already returned to the caller.
    seen: BTreeSet<Address>,
}

/// Backend-specific scan state.
#[derive(Debug)]
enum ScanInner {
    /// Production discovery session.
    Dbus(crate::bus::DbusScan),
    /// In-process scan.
    Fake {
        /// Owning adapter session.
        session: FakeSession,
        /// Lease identifier.
        lease: u64,
        /// Inclusive deadline on the fake clock.
        deadline_ms: u64,
    },
}

impl BleScan {
    /// Builds a fake scan lease.
    #[must_use]
    #[inline]
    pub(crate) fn fake(
        session: FakeSession,
        lease: u64,
        deadline_ms: u64,
    ) -> Self {
        Self {
            inner: ScanInner::Fake {
                session,
                lease,
                deadline_ms,
            },
            seen: BTreeSet::new(),
        }
    }

    /// Builds a production scan lease.
    #[must_use]
    #[inline]
    pub(crate) fn dbus(handle: crate::bus::DbusScan) -> Self {
        Self {
            inner: ScanInner::Dbus(handle),
            seen: BTreeSet::new(),
        }
    }

    /// Returns the next unseen receiver, if the deadline has not elapsed.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] with [`crate::ErrorKind::Timeout`] when the deadline
    /// elapses without a candidate, or a bus error from BlueZ.
    #[inline]
    pub fn next_candidate(&mut self) -> Result<Option<BleCandidate>, Error> {
        match &mut self.inner {
            ScanInner::Dbus(handle) => handle.next_candidate(&mut self.seen),
            ScanInner::Fake {
                session,
                deadline_ms,
                ..
            } => next_fake_candidate(session, *deadline_ms, &mut self.seen),
        }
    }

    /// Stops discovery.
    ///
    /// # Errors
    ///
    /// Returns an error when BlueZ rejects `StopDiscovery`.
    #[inline]
    pub fn stop(mut self) -> Result<(), Error> {
        self.unregister()
    }

    /// Releases the lease once.
    fn unregister(&mut self) -> Result<(), Error> {
        match &mut self.inner {
            ScanInner::Dbus(handle) => handle.stop(),
            ScanInner::Fake { session, lease, .. } => {
                let mut radio = lock(&session.radio)?;
                if let Some(slot) = radio.adapters.get_mut(&session.address) {
                    let _removed = slot.scans.remove(lease);
                }
                Ok(())
            }
        }
    }
}

impl Drop for BleScan {
    #[inline]
    fn drop(&mut self) {
        let _result = self.unregister();
    }
}

impl FakeSession {
    /// Starts a fake BLE scan.
    pub(crate) fn scan_ble(
        &self,
        deadline: Duration,
    ) -> Result<BleScan, Error> {
        let mut radio = lock(&self.radio)?;
        let now_ms = radio.now_ms;
        let lease = next_lease(&mut radio.next_lease);
        let slot = powered_slot(&mut radio.adapters, self.address)?;
        let _inserted = slot.scans.insert(lease);
        let deadline_ms = now_ms.saturating_add(as_millis(deadline));
        Ok(BleScan::fake(self.clone(), lease, deadline_ms))
    }
}

/// Polls other adapters' receiver advertisements.
fn next_fake_candidate(
    session: &FakeSession,
    deadline_ms: u64,
    seen: &mut BTreeSet<Address>,
) -> Result<Option<BleCandidate>, Error> {
    let radio = lock(&session.radio)?;
    if let Some(candidate) = find_candidate(&radio, session.address, seen) {
        return Ok(Some(candidate));
    }
    if radio.now_ms >= deadline_ms {
        Err(Error::timeout())
    } else {
        Ok(None)
    }
}

/// Finds one unseen advertisement from another powered adapter.
fn find_candidate(
    radio: &RadioInner,
    local: Address,
    seen: &mut BTreeSet<Address>,
) -> Option<BleCandidate> {
    for (address, slot) in &radio.adapters {
        if *address == local || !slot.powered || seen.contains(address) {
            continue;
        }
        if let Some(advertisement) = slot.advertisements.values().next() {
            let _inserted = seen.insert(*address);
            return Some(BleCandidate::new(
                *address,
                advertisement.service_data().to_vec(),
            ));
        }
    }
    None
}

/// Converts a duration to whole milliseconds.
fn as_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Marks `slot` as having an active scan. Used by Classic discovery.
#[expect(
    dead_code,
    reason = "Classic discovery shares powered-slot checks in sibling modules"
)]
pub(crate) fn require_powered(slot: &AdapterSlot) -> Result<(), Error> {
    if slot.powered {
        Ok(())
    } else {
        Err(Error::unavailable("adapter is not powered"))
    }
}
