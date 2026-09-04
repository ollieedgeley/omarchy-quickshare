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
    let bluetooth = Adapter::system().ok();
    let manager = NetworkManager::system().ok();
    let mut browser: Option<Browser> = None;
    let mut discovering = false;
    let mut inbound: Option<PublishedLanListener> = None;
    let mut visibility = VisibilityLeases::default();
    let mut discovery = DiscoveryLeases::default();
    let mut restart_at = Instant::now();
    let mut seen = HashSet::new();
    loop {
        if discovering && (browser.is_none() || Instant::now() >= restart_at) {
            if let Some(active_browser) = browser.take() {
                let _result = active_browser.stop();
            }
            browser = dns_sd.browse(MdnsInstance::service_type()).ok();
            let now = Instant::now();
            restart_at = now.checked_add(BROWSE_RETRY_INTERVAL).unwrap_or(now);
        }
        if let Some(active_browser) = browser.as_ref()
            && let Ok(Some(service)) = active_browser.resolve(POLL_INTERVAL)
            && let Some(event) = discovered_peer(&service)
        {
            remember_seen(&mut seen, &event);
            if events.send(event).is_err() {
                break;
            }
        }
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
                        bluetooth.as_ref(),
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
                        bluetooth.as_ref(),
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
            bluetooth.as_ref(),
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
    bluetooth: Option<&Adapter>,
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
            *discovery = start_discovery(bluetooth, DISCOVERY_LEASE);
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
            *visibility = open_visibility(bluetooth);
            true
        }
        NetworkCommand::SendShare { share_id, transfer } => events
            .send(outbound_event(
                share_id,
                &transfer,
                events,
                cancellation,
                bluetooth,
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
