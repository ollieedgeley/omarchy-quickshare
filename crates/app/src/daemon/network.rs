//! Background LAN discovery and transfer I/O owned by the production daemon.

#![expect(
    clippy::pub_with_shorthand,
    reason = "rustfmt scoped visibility conflicts with the restriction lint"
)]
#![expect(
    clippy::single_call_fn,
    reason = "Named worker stages keep scheduling and protocol I/O separate"
)]

mod inbound;

use core::net::SocketAddrV4;
use core::time::Duration;
use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread;
use std::time::Instant;

use quickshare_network::{
    Browser, DnsSd, ResolvedService,
    lan::{PublishedLanListener, connect},
};
use quickshare_sharing::{EndpointInfo, MdnsInstance, SharingSession};
use quickshare_storage::OutboundSource;

use self::inbound::{open_listener, receive_file};
use super::outbound::OutboundTransfer;

/// Maximum wait before processing another worker command.
const POLL_INTERVAL: Duration = Duration::from_millis(5);
/// Time between mDNS browse restarts while discovery remains requested.
const BROWSE_RETRY_INTERVAL: Duration = Duration::from_secs(1);
/// Connections endpoint identifier used by the first local daemon identity.
const ENDPOINT_ID: &str = "OQSR";
/// User-visible endpoint name used by the first local daemon identity.
const ENDPOINT_NAME: &str = "Omarchy";

/// Commands sent from the local-control owner to the network worker.
#[derive(Debug)]
pub(super) enum NetworkCommand {
    /// Accepts the currently offered inbound file.
    AcceptInbound {
        /// Stable local share identifier assigned after the offer appeared.
        share_id: u64,
    },
    /// Stops advertising this endpoint to nearby senders.
    CloseVisibility,
    /// Starts Nearby Sharing LAN discovery.
    Discover,
    /// Advertises this endpoint and listens for one incoming connection.
    OpenVisibility,
    /// Sends one queued file to its selected LAN peer.
    SendFile {
        /// Stable local share identifier.
        share_id: u64,
        /// Resolved file path and peer route.
        transfer: OutboundTransfer,
    },
}

/// Observations sent from the network worker to the local-control owner.
#[derive(Debug)]
pub(super) enum NetworkEvent {
    /// Every byte of an accepted inbound file was saved locally.
    InboundCompleted {
        /// Number of file bytes saved.
        bytes: u64,
        /// Stable local share identifier.
        share_id: u64,
    },
    /// An inbound connection or transfer failed.
    InboundFailed {
        /// Internal failure detail written to the daemon log.
        reason: String,
        /// Share identifier when local consent had already been given.
        share_id: Option<u64>,
    },
    /// A validated inbound file is waiting for local consent.
    InboundOffered {
        /// Safe file basename advertised by the peer.
        name: String,
        /// Declared file byte length.
        size_bytes: u64,
    },
    /// A selected peer accepted and received every file byte.
    OutboundCompleted {
        /// Number of file bytes sent.
        bytes: u64,
        /// Stable local share identifier.
        share_id: u64,
    },
    /// A selected peer connection or transfer failed.
    OutboundFailed {
        /// Internal failure detail written to the daemon log.
        reason: String,
        /// Stable local share identifier.
        share_id: u64,
    },
    /// A valid Nearby Sharing peer appeared on the LAN.
    PeerSeen {
        /// User-visible name from the endpoint-info advertisement.
        name: String,
        /// Opaque Nearby endpoint identifier for later connection setup.
        peer_id: String,
        /// Private TCP route advertised through DNS-SD.
        route: SocketAddrV4,
    },
}

/// A background worker that owns Linux network adapters for the daemon.
#[derive(Debug)]
pub(super) struct NetworkWorker {
    /// Commands accepted from the control-loop owner.
    commands: Option<Sender<NetworkCommand>>,
    /// Peer observations delivered to the control-loop owner.
    events: Receiver<NetworkEvent>,
    /// Background thread that stops after its command sender is dropped.
    worker: Option<thread::JoinHandle<()>>,
}

#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "Worker commands are grouped before event polling and startup"
)]
impl NetworkWorker {
    /// Accepts the inbound offer currently waiting in the worker.
    pub(super) fn accept_inbound(&self, share_id: u64) -> io::Result<()> {
        self.send(NetworkCommand::AcceptInbound { share_id })
    }

    /// Stops advertising this endpoint to nearby senders.
    pub(super) fn close_visibility(&self) -> io::Result<()> {
        self.send(NetworkCommand::CloseVisibility)
    }

    /// Requests one Nearby Sharing LAN browse.
    pub(super) fn discover(&self) -> io::Result<()> {
        self.send(NetworkCommand::Discover)
    }

    /// Advertises this endpoint and listens for an incoming connection.
    pub(super) fn open_visibility(&self) -> io::Result<()> {
        self.send(NetworkCommand::OpenVisibility)
    }

    /// Returns the next completed network observation without waiting.
    pub(super) fn next_event(&self) -> io::Result<Option<NetworkEvent>> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "network worker stopped",
            )),
        }
    }

    /// Queues one selected file for an encrypted outbound transfer.
    pub(super) fn send_file(
        &self,
        share_id: u64,
        transfer: OutboundTransfer,
    ) -> io::Result<()> {
        self.send(NetworkCommand::SendFile { share_id, transfer })
    }

    /// Sends one command after confirming that the worker remains available.
    fn send(&self, command: NetworkCommand) -> io::Result<()> {
        self.commands
            .as_ref()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "network worker stopped",
                )
            })?
            .send(command)
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))
    }

    #[expect(
        clippy::single_call_fn,
        reason = "Daemon startup owns the one production worker construction"
    )]
    /// Starts the production network worker after its DNS-SD adapter is ready.
    pub(super) fn start() -> io::Result<Self> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) =
            mpsc::channel::<Result<(), String>>();
        let worker = thread::spawn(move || {
            let dns_sd = match DnsSd::new() {
                Ok(dns_sd) => {
                    let _result = ready_sender.send(Ok(()));
                    dns_sd
                }
                Err(error) => {
                    let _result = ready_sender.send(Err(error.to_string()));
                    return;
                }
            };
            run_worker(dns_sd, command_receiver, event_sender);
        });
        ready_receiver
            .recv()
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))?
            .map_err(io::Error::other)?;
        Ok(Self {
            commands: Some(command_sender),
            events: event_receiver,
            worker: Some(worker),
        })
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Worker shutdown needs only Drop's destructor hook"
)]
impl Drop for NetworkWorker {
    fn drop(&mut self) {
        drop(self.commands.take());
        if let Some(worker) = self.worker.take() {
            drop(worker.join());
        }
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "The detached worker owns its DNS-SD adapter and channels"
)]
#[expect(
    clippy::single_call_fn,
    reason = "The worker loop is named for its long-running lifecycle"
)]
/// Owns DNS-SD browsing until the daemon drops its command channel.
fn run_worker(
    dns_sd: DnsSd,
    commands: Receiver<NetworkCommand>,
    events: Sender<NetworkEvent>,
) {
    let mut browser: Option<Browser> = None;
    let mut discovering = false;
    let mut inbound: Option<PublishedLanListener> = None;
    let mut restart_at = Instant::now();
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
            && events.send(event).is_err()
        {
            return;
        }
        if let Some(listener) = inbound.as_ref()
            && let Ok(Some(stream)) = listener.accept()
        {
            let event = receive_file(stream, &commands, &events);
            if events.send(event).is_err() {
                return;
            }
        }
        let command = match commands.recv_timeout(POLL_INTERVAL) {
            Ok(command) => command,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if !handle_command(
            command,
            &dns_sd,
            &events,
            &mut discovering,
            &mut restart_at,
            &mut inbound,
        ) {
            return;
        }
    }
}

/// Applies one nonblocking worker command and reports channel availability.
fn handle_command(
    command: NetworkCommand,
    dns_sd: &DnsSd,
    events: &Sender<NetworkEvent>,
    discovering: &mut bool,
    restart_at: &mut Instant,
    inbound: &mut Option<PublishedLanListener>,
) -> bool {
    match command {
        NetworkCommand::AcceptInbound { .. } => true,
        NetworkCommand::CloseVisibility => {
            if let Some(listener) = inbound.take() {
                let _result = listener.stop();
            }
            true
        }
        NetworkCommand::Discover => {
            *discovering = true;
            *restart_at = Instant::now();
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
            true
        }
        NetworkCommand::SendFile { share_id, transfer } => {
            events.send(outbound_event(share_id, &transfer)).is_ok()
        }
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "The decoder separates wire validation from worker scheduling"
)]
/// Decodes the Nearby Sharing facts that the daemon exposes to local control.
fn discovered_peer(service: &ResolvedService) -> Option<NetworkEvent> {
    let instance = MdnsInstance::decode_label(service.instance()).ok()?;
    let endpoint =
        EndpointInfo::decode_property(service.property("n")?).ok()?;
    let name = endpoint.device_name()?.to_owned();
    let address = service.addresses().first().copied()?;
    Some(NetworkEvent::PeerSeen {
        name,
        peer_id: instance.label(),
        route: SocketAddrV4::new(address, service.port()),
    })
}

/// Converts one worker transfer attempt into a terminal daemon event.
fn outbound_event(share_id: u64, transfer: &OutboundTransfer) -> NetworkEvent {
    match send_file(transfer.source(), transfer.route()) {
        Ok(bytes) => NetworkEvent::OutboundCompleted { bytes, share_id },
        Err(error) => NetworkEvent::OutboundFailed {
            reason: error.to_string(),
            share_id,
        },
    }
}

/// Sends one complete file over an encrypted account-free session.
fn send_file(source: &OutboundSource, route: SocketAddrV4) -> io::Result<u64> {
    let name = source.name().to_str().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "file name is not UTF-8")
    })?;
    let bytes = source.read_all().map_err(io::Error::other)?;
    let stream = connect(route)?;
    let mut session =
        SharingSession::connect(stream, ENDPOINT_ID, ENDPOINT_NAME)
            .map_err(io::Error::other)?;
    let _pairing = session
        .exchange_account_free_pairing()
        .map_err(io::Error::other)?;
    session
        .send_outgoing_file(name, &bytes)
        .map_err(io::Error::other)?;
    u64::try_from(bytes.len()).map_err(io::Error::other)
}
