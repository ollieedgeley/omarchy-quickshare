use alloc::collections::BTreeSet;
use core::time::Duration;

use super::{
    ClassicCandidate, ClassicDiscovery, ClassicListener, ClassicSocket,
};
use crate::advertisement::{powered_slot, service_uuid};
use crate::radio::{Address, Error, FakeSession, lock, next_lease};
use crate::token;

impl FakeSession {
    /// Connects to a Classic listener.
    pub(crate) fn connect_classic(
        &self,
        candidate: &ClassicCandidate,
        service_uuid: &str,
        deadline: Duration,
    ) -> Result<ClassicSocket, Error> {
        let mut radio = lock(&self.radio)?;
        if deadline_elapsed(&radio, deadline) {
            return Err(Error::timeout());
        }
        let peer = radio
            .adapters
            .get_mut(&candidate.address())
            .ok_or_else(|| Error::unavailable("peer adapter is missing"))?;
        if !peer.powered || !peer.classic_listeners.contains(service_uuid) {
            return Err(Error::unavailable("peer Classic is not listening"));
        }
        peer.classic_inbox
            .entry(String::from(service_uuid))
            .or_default()
            .push_back(token::encode(self.address));
        Ok(ClassicSocket::fake(
            self.clone(),
            candidate.address(),
            String::from(service_uuid),
        ))
    }

    /// Starts Classic inquiry on the fake radio.
    pub(crate) fn discover_classic(
        &self,
        deadline: Duration,
    ) -> Result<ClassicDiscovery, Error> {
        let mut radio = lock(&self.radio)?;
        let now_ms = radio.now_ms;
        let lease = next_lease(&mut radio.next_lease);
        let slot = powered_slot(&mut radio.adapters, self.address)?;
        let _inserted = slot.classic_scans.insert(lease);
        Ok(ClassicDiscovery::fake(
            self.clone(),
            lease,
            now_ms.saturating_add(as_millis(deadline)),
        ))
    }

    /// Registers a fake Classic listener.
    pub(crate) fn listen_classic(
        &self,
        service_uuid: &str,
    ) -> Result<ClassicListener, Error> {
        let mut radio = lock(&self.radio)?;
        let slot = powered_slot(&mut radio.adapters, self.address)?;
        if !slot.classic_listeners.insert(String::from(service_uuid)) {
            return Err(Error::protocol("Classic UUID already bound"));
        }
        Ok(ClassicListener::fake(
            self.clone(),
            String::from(service_uuid),
        ))
    }
}

pub(super) fn next_fake_candidate(
    session: &FakeSession,
    deadline_ms: u64,
    seen: &mut BTreeSet<Address>,
) -> Result<Option<ClassicCandidate>, Error> {
    let radio = lock(&session.radio)?;
    for (address, slot) in &radio.adapters {
        if *address == session.address
            || !slot.powered
            || seen.contains(address)
        {
            continue;
        }
        if !slot
            .classic_listeners
            .iter()
            .any(|uuid| uuid.eq_ignore_ascii_case(service_uuid()))
        {
            continue;
        }
        let _inserted = seen.insert(*address);
        return Ok(Some(ClassicCandidate::new(*address)));
    }
    if radio.now_ms >= deadline_ms {
        Err(Error::timeout())
    } else {
        Ok(None)
    }
}

pub(super) fn accept_fake(
    session: &FakeSession,
    uuid: &str,
) -> Result<Option<ClassicSocket>, Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&session.address)
        .ok_or_else(|| Error::closed())?;
    let Some(inbox) = slot.classic_inbox.get_mut(uuid) else {
        return Ok(None);
    };
    let Some(raw) = inbox.pop_front() else {
        return Ok(None);
    };
    let peer = token::parse(&raw)?;
    Ok(Some(ClassicSocket::fake(
        session.clone(),
        peer,
        String::from(uuid),
    )))
}

pub(super) fn send_fake(
    session: &FakeSession,
    peer: Address,
    uuid: &str,
    bytes: &[u8],
) -> Result<(), Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&peer)
        .ok_or_else(|| Error::unavailable("peer adapter is missing"))?;
    slot.classic_inbox
        .entry(String::from(uuid))
        .or_default()
        .push_back(bytes.to_vec());
    Ok(())
}

pub(super) fn recv_fake(
    session: &FakeSession,
    uuid: &str,
    deadline: Duration,
) -> Result<Vec<u8>, Error> {
    let mut radio = lock(&session.radio)?;
    let slot = radio
        .adapters
        .get_mut(&session.address)
        .ok_or_else(|| Error::closed())?;
    if let Some(inbox) = slot.classic_inbox.get_mut(uuid) {
        if let Some(bytes) = token::pop_payload(inbox) {
            return Ok(bytes);
        }
    }
    if deadline_elapsed(&radio, deadline) {
        Err(Error::timeout())
    } else {
        Err(Error::timeout())
    }
}

fn as_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn deadline_elapsed(
    radio: &crate::radio::RadioInner,
    deadline: Duration,
) -> bool {
    radio.now_ms >= radio.now_ms.saturating_add(as_millis(deadline))
}
