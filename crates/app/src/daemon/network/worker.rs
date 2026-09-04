//! Background DNS-SD, Bluetooth, and inbound scheduling for the network worker.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Instant;

use super::inbound::{open_listener, receive_share};
use super::transfer::outbound_event;
use super::{NetworkCommand, NetworkEvent, TransferCancellation};
use crate::daemon::media::{
    DiscoveryLeases, PeerRoute, VisibilityLeases, open_visibility,
    start_discovery,
};
use core::time::Duration;
use quickshare_bluez::Adapter;
use quickshare_connections::Medium;
use quickshare_network::{
    Browser, DnsSd, NetworkManager, ResolvedService, lan::PublishedLanListener,
};
use quickshare_sharing::{EndpointInfo, MdnsInstance};

/// Maximum wait before processing another worker command.
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Time between mDNS browse restarts while discovery remains requested.
const BROWSE_RETRY_INTERVAL: Duration = Duration::from_secs(1);
/// BLE and Classic discovery lease lifetime while searching.
const DISCOVERY_LEASE: Duration = Duration::from_secs(15);
/// Minimum delay between full BlueZ object-tree snapshots.
const BLUETOOTH_POLL_INTERVAL: Duration = Duration::from_millis(250);

#[expect(
    clippy::needless_pass_by_value,
    reason = "The detached worker owns its DNS-SD adapter and channels"
)]
#[expect(
    clippy::single_call_fn,
    reason = "The worker loop is named for its long-running lifecycle"
)]
/// Owns DNS-SD browsing until the daemon drops its command channel.
pub(super) fn run_worker(
    dns_sd: DnsSd,
    commands: Receiver<NetworkCommand>,
    events: Sender<NetworkEvent>,
    cancellation: TransferCancellation,
    receive_directory: PathBuf,
    consent_deadline: Duration,
) {
    let mut bluetooth = system_stage("bluez_adapter", Adapter::system());
    let manager = system_stage("network_manager", NetworkManager::system());
    let mut browser: Option<Browser> = None;
    let mut discovering = false;
    let mut inbound: Option<PublishedLanListener> = None;
    let mut visibility = VisibilityLeases::default();
    let mut discovery = DiscoveryLeases::default();
    let mut restart_at = Instant::now();
    let mut next_bluetooth_poll = Instant::now();
    let mut seen = HashSet::new();
    let mut mdns_browse_ok = None;
    let mut advertisement_decode_failed = false;
    loop {
        if discovering && (browser.is_none() || Instant::now() >= restart_at) {
            restart_browser(
                &dns_sd,
                &mut browser,
                &mut mdns_browse_ok,
                &mut restart_at,
            );
        }
        if let Some(active_browser) = browser.as_ref()
            && let Ok(Some(service)) = active_browser.resolve(POLL_INTERVAL)
            && !emit_resolved_peer(
                &service,
                &mut seen,
                &events,
                &mut advertisement_decode_failed,
            )
        {
            break;
        }
        let now = Instant::now();
        if now >= next_bluetooth_poll {
            next_bluetooth_poll =
                now.checked_add(BLUETOOTH_POLL_INTERVAL).unwrap_or(now);
            if let Some(sighting) = discovery.next_sighting() {
                let event = NetworkEvent::PeerSeen {
                    name: sighting.name,
                    peer_id: sighting.peer_id,
                    route: sighting.route,
                };
                remember_seen(&mut seen, &event);
                if events.send(event).is_err() {
                    break;
                }
            }
        }
        if let Some(stream) = inbound
            .as_ref()
            .and_then(|listener| listener.accept().ok().flatten())
        {
            let event = receive_share(
                stream,
                Medium::WifiLan,
                &commands,
                &events,
                &cancellation,
                &receive_directory,
                consent_deadline,
                manager.as_ref(),
                &mut |command| {
                    handle_command(
                        command,
                        &dns_sd,
                        &mut bluetooth,
                        manager.as_ref(),
                        &events,
                        &cancellation,
                        &mut discovering,
                        &mut restart_at,
                        &mut inbound,
                        &mut visibility,
                        &mut discovery,
                        &mut browser,
                        &mut seen,
                    )
                },
            );
            if events.send(event).is_err() {
                break;
            }
        }
        if let Some((stream, medium)) = visibility.accept_next() {
            let event = receive_share(
                stream,
                medium,
                &commands,
                &events,
                &cancellation,
                &receive_directory,
                consent_deadline,
                manager.as_ref(),
                &mut |command| {
                    handle_command(
                        command,
                        &dns_sd,
                        &mut bluetooth,
                        manager.as_ref(),
                        &events,
                        &cancellation,
                        &mut discovering,
                        &mut restart_at,
                        &mut inbound,
                        &mut visibility,
                        &mut discovery,
                        &mut browser,
                        &mut seen,
                    )
                },
            );
            if events.send(event).is_err() {
                break;
            }
        }
        let command = match commands.recv_timeout(POLL_INTERVAL) {
            Ok(command) => command,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        if !handle_command(
            command,
            &dns_sd,
            &mut bluetooth,
            manager.as_ref(),
            &events,
            &cancellation,
            &mut discovering,
            &mut restart_at,
            &mut inbound,
            &mut visibility,
            &mut discovery,
            &mut browser,
            &mut seen,
        ) {
            break;
        }
    }
    tracing::debug!(stage = "network_worker", "network worker stopped");
    core::mem::take(&mut discovery).close();
    core::mem::take(&mut visibility).close();
    if let Some(active_browser) = browser.take() {
        let _result = active_browser.stop();
    }
}

/// Applies one nonblocking worker command and reports channel availability.
#[expect(
    clippy::too_many_arguments,
    reason = "Worker commands keep adapter, lease, and channel owners"
)]
fn handle_command(
    command: NetworkCommand,
    dns_sd: &DnsSd,
    bluetooth: &mut Option<Adapter>,
    manager: Option<&NetworkManager>,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
    discovering: &mut bool,
    restart_at: &mut Instant,
    inbound: &mut Option<PublishedLanListener>,
    visibility: &mut VisibilityLeases,
    discovery: &mut DiscoveryLeases,
    browser: &mut Option<Browser>,
    seen: &mut HashSet<String>,
) -> bool {
    match command {
        NetworkCommand::AcceptInbound { .. }
        | NetworkCommand::RejectInbound { .. } => true,
        NetworkCommand::CloseVisibility => {
            if let Some(listener) = inbound.take() {
                let _result = listener.stop();
            }
            core::mem::take(visibility).close();
            true
        }
        NetworkCommand::Discover => {
            *discovering = true;
            *restart_at = Instant::now();
            core::mem::take(discovery).close();
            *discovery =
                start_discovery(refresh_adapter(bluetooth), DISCOVERY_LEASE);
            true
        }
        NetworkCommand::OpenVisibility => {
            if inbound.is_none() {
                match open_listener(dns_sd) {
                    Ok(listener) => *inbound = Some(listener),
                    Err(error) => {
                        return events
                            .send(NetworkEvent::InboundFailed {
                                reason: error.to_string(),
                                share_id: None,
                            })
                            .is_ok();
                    }
                }
            }
            core::mem::take(visibility).close();
            *visibility = open_visibility(refresh_adapter(bluetooth));
            true
        }
        NetworkCommand::SendShare { share_id, transfer } => events
            .send(outbound_event(
                share_id,
                &transfer,
                events,
                cancellation,
                bluetooth.as_ref(),
                manager,
            ))
            .is_ok(),
        NetworkCommand::StopDiscovery => {
            *discovering = false;
            if let Some(active_browser) = browser.take() {
                let _result = active_browser.stop();
            }
            core::mem::take(discovery).close();
            emit_peer_lost(seen, events)
        }
    }
}

fn refresh_adapter(bluetooth: &mut Option<Adapter>) -> Option<&Adapter> {
    refresh_adapter_with(bluetooth, Adapter::system)
}

fn refresh_adapter_with<Open, OpenError>(
    bluetooth: &mut Option<Adapter>,
    open: Open,
) -> Option<&Adapter>
where
    Open: FnOnce() -> Result<Adapter, OpenError>,
{
    let previous = bluetooth.is_some();
    match open() {
        Ok(adapter) => {
            if !previous {
                tracing::debug!(
                    stage = "bluez_adapter",
                    available = true,
                    "adapter stage ready"
                );
            }
            *bluetooth = Some(adapter);
        }
        Err(_) => {
            if previous {
                tracing::warn!(
                    stage = "bluez_adapter",
                    available = false,
                    "adapter stage failed"
                );
            }
            *bluetooth = None;
        }
    }
    bluetooth.as_ref()
}

fn system_stage<T, E>(stage: &'static str, result: Result<T, E>) -> Option<T> {
    result
        .inspect(|_| {
            tracing::debug!(stage, available = true, "adapter stage ready");
        })
        .inspect_err(|_| {
            tracing::warn!(stage, available = false, "adapter stage failed");
        })
        .ok()
}

fn restart_browser(
    dns_sd: &DnsSd,
    browser: &mut Option<Browser>,
    mdns_browse_ok: &mut Option<bool>,
    restart_at: &mut Instant,
) {
    if let Some(active_browser) = browser.take() {
        let _result = active_browser.stop();
    }
    *browser = match dns_sd.browse(MdnsInstance::service_type()) {
        Ok(started) => {
            if *mdns_browse_ok != Some(true) {
                tracing::debug!(
                    stage = "mdns_browse",
                    available = true,
                    "adapter stage ready"
                );
            }
            *mdns_browse_ok = Some(true);
            Some(started)
        }
        Err(_) => {
            if *mdns_browse_ok != Some(false) {
                tracing::warn!(
                    stage = "mdns_browse",
                    available = false,
                    "adapter stage failed"
                );
            }
            *mdns_browse_ok = Some(false);
            None
        }
    };
    let now = Instant::now();
    *restart_at = now.checked_add(BROWSE_RETRY_INTERVAL).unwrap_or(now);
}

fn emit_resolved_peer(
    service: &ResolvedService,
    seen: &mut HashSet<String>,
    events: &Sender<NetworkEvent>,
    advertisement_decode_failed: &mut bool,
) -> bool {
    let Some(event) = discovered_peer(service) else {
        if !*advertisement_decode_failed {
            *advertisement_decode_failed = true;
            tracing::warn!(
                stage = "advertisement_decode",
                error_class = "invalid_payload",
                "adapter stage failed"
            );
        }
        return true;
    };
    remember_seen(seen, &event);
    events.send(event).is_ok()
}

/// Records a newly advertised peer so stop/timeout can emit PeerLost.
pub(super) fn remember_seen(seen: &mut HashSet<String>, event: &NetworkEvent) {
    if let NetworkEvent::PeerSeen { peer_id, .. } = event {
        let _inserted = seen.insert(peer_id.clone());
    }
}

/// Emits PeerLost for every remembered peer and clears the set.
pub(super) fn emit_peer_lost(
    seen: &mut HashSet<String>,
    events: &Sender<NetworkEvent>,
) -> bool {
    for peer_id in core::mem::take(seen) {
        if events.send(NetworkEvent::PeerLost { peer_id }).is_err() {
            return false;
        }
    }
    true
}

/// Decodes the Nearby Sharing facts that the daemon exposes to local control.
fn discovered_peer(service: &ResolvedService) -> Option<NetworkEvent> {
    use core::net::SocketAddrV4;
    let instance = MdnsInstance::decode_label(service.instance()).ok()?;
    let endpoint =
        EndpointInfo::decode_property(service.property("n")?).ok()?;
    let name = endpoint.device_name()?.to_owned();
    let address = service.addresses().first().copied()?;
    Some(NetworkEvent::PeerSeen {
        name,
        peer_id: instance.label(),
        route: PeerRoute::Lan(SocketAddrV4::new(address, service.port())),
    })
}

#[cfg(test)]
mod tests {
    use super::refresh_adapter_with;
    use quickshare_bluez::testing::FakeRadio;
    use quickshare_bluez::{Adapter, Address};

    #[test]
    fn controller_loss_then_late_availability_refreshes_the_slot() {
        let original = FakeRadio::new()
            .adapter(Address::from_bytes([2, 0, 0, 0, 0, 1]))
            .expect("original adapter");
        let replacement = FakeRadio::new()
            .adapter(Address::from_bytes([2, 0, 0, 0, 0, 2]))
            .expect("replacement adapter");
        let mut bluetooth = Some(original);

        let missing =
            refresh_adapter_with(&mut bluetooth, || Err::<Adapter, ()>(()));
        assert!(missing.is_none());
        let recovered =
            refresh_adapter_with(&mut bluetooth, || Ok::<_, ()>(replacement));

        assert!(recovered.is_some());
        assert!(bluetooth.is_some());
    }
}
