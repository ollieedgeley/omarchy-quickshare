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
mod transfer;
mod worker;

#[cfg(test)]
mod tests;

use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use self::worker::run_worker;
#[cfg(test)]
use self::worker::{emit_peer_lost, remember_seen};
use super::media::PeerRoute;
use super::outbound::OutboundTransfer;
use quickshare_network::DnsSd;

/// Commands sent from the local-control owner to the network worker.
#[derive(Debug)]
pub(super) enum NetworkCommand {
    /// Accepts the currently offered inbound attachment.
    AcceptInbound {
        /// Stable local share identifier assigned after the offer appeared.
        share_id: u64,
    },
    /// Stops advertising this endpoint to nearby senders.
    CloseVisibility,
    /// Starts Nearby Sharing LAN, BLE, and Classic discovery.
    Discover,
    /// Advertises this endpoint and listens for incoming connections.
    OpenVisibility,
    /// Rejects the currently offered inbound attachment.
    RejectInbound {
        /// Stable local share identifier assigned after the offer appeared.
        share_id: u64,
    },
    /// Sends one queued share to its selected peer.
    SendShare {
        /// Stable local share identifier.
        share_id: u64,
        /// Resolved payload and peer routes.
        transfer: OutboundTransfer,
    },
    /// Stops LAN, BLE, and Classic discovery and releases leases.
    StopDiscovery,
}

/// Observations sent from the network worker to the local-control owner.
#[derive(Debug)]
pub(super) enum NetworkEvent {
    /// Either endpoint cancelled an inbound transfer.
    InboundCancelled {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Every byte of an accepted inbound attachment was saved locally.
    InboundCompleted {
        /// Number of payload bytes saved.
        bytes: u64,
        /// Attachment kind received.
        kind: quickshare_sharing::OfferKind,
        /// Stable local share identifier.
        share_id: u64,
        /// Exact text or URL bytes when the attachment is not a file.
        value: Option<String>,
    },
    /// An inbound connection or transfer failed.
    InboundFailed {
        /// Internal failure detail written to the daemon log.
        reason: String,
        /// Share identifier when local consent had already been given.
        share_id: Option<u64>,
    },
    /// A validated inbound attachment is waiting for local consent.
    InboundOffered {
        /// Attachment kind advertised by the peer.
        kind: quickshare_sharing::OfferKind,
        /// Safe file basename or text title advertised by the peer.
        name: String,
        /// Declared payload byte length.
        size_bytes: u64,
        /// Four-digit code derived from the shared authentication token.
        verification_code: String,
    },
    /// The local user rejected an inbound offer.
    InboundRejected {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// The remote peer accepted the offered attachment.
    OutboundAccepted {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// Either endpoint cancelled an outbound transfer.
    OutboundCancelled {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// A selected peer accepted and received every payload byte.
    OutboundCompleted {
        /// Number of payload bytes sent.
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
    /// UKEY2 established a code for the peer-consent prompt.
    OutboundPairing {
        /// Stable local share identifier.
        share_id: u64,
        /// Four-digit code derived from the shared authentication token.
        verification_code: String,
    },
    /// The remote peer rejected the outbound offer.
    OutboundRejected {
        /// Stable local share identifier.
        share_id: u64,
    },
    /// A previously visible peer is no longer advertised.
    PeerLost {
        /// Opaque Nearby endpoint identifier.
        peer_id: String,
    },
    /// A valid Nearby Sharing peer appeared on a local medium.
    PeerSeen {
        /// User-visible name from the endpoint-info advertisement.
        name: String,
        /// Opaque Nearby endpoint identifier for later connection setup.
        peer_id: String,
        /// Private candidate route for this sighting.
        route: PeerRoute,
    },
    /// Payload bytes observed for the active share.
    Progress {
        /// Selected medium carrying these bytes.
        medium: String,
        /// Stable local share identifier.
        share_id: u64,
        /// Total bytes observed at the transfer seam.
        transferred_bytes: u64,
    },
}

/// Lock-free cancellation signal shared with active transfer I/O.
#[derive(Clone, Debug, Default)]
struct TransferCancellation {
    /// Stable identifier of the share requested for cancellation.
    share_id: Arc<AtomicU64>,
}

impl TransferCancellation {
    /// Marks one share for cancellation.
    fn cancel(&self, share_id: u64) {
        self.share_id.store(share_id, Ordering::Release);
    }

    /// Clears one completed share without overwriting a newer cancellation.
    fn finish(&self, share_id: u64) {
        let _result = self.share_id.compare_exchange(
            share_id,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Reports whether this share owns the current cancellation request.
    fn is_cancelled(&self, share_id: u64) -> bool {
        self.share_id.load(Ordering::Acquire) == share_id
    }
}

/// A background worker that owns Linux network adapters for the daemon.
#[derive(Debug)]
pub(super) struct NetworkWorker {
    /// Lock-free cancellation observed between encrypted transfer frames.
    cancellation: TransferCancellation,
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
    /// Requests cancellation at the next encrypted transfer-frame boundary.
    pub(super) fn cancel_transfer(&self, share_id: u64) {
        self.cancellation.cancel(share_id);
    }

    /// Stops advertising this endpoint to nearby senders.
    pub(super) fn close_visibility(&self) -> io::Result<()> {
        self.send(NetworkCommand::CloseVisibility)
    }

    /// Requests one Nearby Sharing LAN browse.
    pub(super) fn discover(&self) -> io::Result<()> {
        self.send(NetworkCommand::Discover)
    }

    /// Stops Nearby Sharing discovery and closes discovery leases.
    pub(super) fn stop_discovery(&self) -> io::Result<()> {
        self.send(NetworkCommand::StopDiscovery)
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

    /// Queues one selected share for an encrypted outbound transfer.
    pub(super) fn send_share(
        &self,
        share_id: u64,
        transfer: OutboundTransfer,
    ) -> io::Result<()> {
        self.send(NetworkCommand::SendShare { share_id, transfer })
    }

    /// Rejects the inbound offer currently waiting in the worker.
    pub(super) fn reject_inbound(&self, share_id: u64) -> io::Result<()> {
        self.send(NetworkCommand::RejectInbound { share_id })
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
    pub(super) fn start(
        receive_directory: PathBuf,
        consent_deadline: Duration,
    ) -> io::Result<Self> {
        let (command_sender, command_receiver) = mpsc::channel();
        let (event_sender, event_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) =
            mpsc::channel::<Result<(), String>>();
        let cancellation = TransferCancellation::default();
        let worker_cancellation = cancellation.clone();
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
            run_worker(
                dns_sd,
                command_receiver,
                event_sender,
                worker_cancellation,
                receive_directory,
                consent_deadline,
            );
        });
        ready_receiver
            .recv()
            .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))?
            .map_err(io::Error::other)?;
        Ok(Self {
            cancellation,
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
