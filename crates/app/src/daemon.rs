//! Local endpoint lifecycle and outbound queue ownership.

mod network;
mod outbound;
mod production;

use core::time::Duration;
use std::io::{self, BufReader};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::{fs, os::unix::net::UnixStream};

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_request, write_response};
use quickshare_control::request::{Envelope as RequestEnvelope, Request};
use quickshare_control::response::{Envelope as ResponseEnvelope, Response};
use quickshare_sharing::{Attachment, Coordinator};
use quickshare_storage::OutboundSource;

use self::network::NetworkWorker;
use self::outbound::OutboundState;

/// Owner-only mode for the control socket directory.
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
/// Owner-only mode for the control socket.
const PRIVATE_SOCKET_MODE: u32 = 0o600;

/// The same-user local endpoint state.
#[derive(Debug, Default)]
pub struct Daemon {
    /// Production network worker, omitted by in-process and simulated daemons.
    network: Option<NetworkWorker>,
    /// Production-only file paths and discovered LAN routes.
    outbound: OutboundState,
    /// Outbound shares accepted from local clients.
    queued: Vec<RequestEnvelope>,
    /// User-visible share lifecycle state.
    sharing: Coordinator,
    /// Whether deterministic peer events are accepted.
    simulated: bool,
}

/// A bound control listener that removes its socket on a clean shutdown.
#[derive(Debug)]
struct ControlSocket {
    /// The listener served by the local endpoint.
    listener: UnixListener,
    /// The filesystem entry removed when the listener is dropped.
    path: PathBuf,
}

impl ControlSocket {
    /// Binds an owner-only socket after rejecting a running endpoint.
    fn bind(path: &Path) -> io::Result<Self> {
        let directory = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "control socket has no parent directory",
            )
        })?;
        fs::create_dir_all(directory)?;
        fs::set_permissions(
            directory,
            fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
        )?;
        remove_stale_socket(path)?;
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(PRIVATE_SOCKET_MODE),
        )?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
        })
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for ControlSocket {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.path));
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Production network behavior stays in its owned daemon submodule"
)]
impl Daemon {
    /// Applies endpoint discovery and visibility controls.
    #[expect(
        clippy::pattern_type_mismatch,
        clippy::wildcard_enum_match_arm,
        reason = "Borrowed non-exhaustive requests require an unhandled case"
    )]
    fn endpoint_response(
        &mut self,
        request: &Request,
    ) -> io::Result<Option<ResponseEnvelope>> {
        match request {
            Request::CloseVisibility => {
                self.sharing.close_visibility();
                if let Some(network) = &self.network {
                    network.close_visibility()?;
                }
            }
            Request::Discover => {
                self.sharing.start_discovery();
                if let Some(network) = &self.network {
                    network.discover()?;
                }
            }
            Request::OpenVisibility => {
                self.sharing.open_visibility();
                if let Some(network) = &self.network {
                    network.open_visibility()?;
                }
            }
            Request::StopDiscovery => self.sharing.stop_discovery(),
            _ => return Ok(None),
        }
        Ok(Some(ResponseEnvelope::applied()))
    }

    /// Creates an empty local endpoint.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            network: None,
            outbound: OutboundState::default(),
            queued: Vec::new(),
            sharing: Coordinator::new(),
            simulated: false,
        }
    }

    /// Queues one validated attachment and reports its stable identifier.
    fn queue_attachment(&mut self, attachment: Attachment) -> u64 {
        self.sharing.queue_outbound(attachment).get()
    }

    /// Returns the number of outbound shares owned by the endpoint.
    #[must_use]
    #[inline]
    pub const fn queued_count(&self) -> usize {
        self.queued.len()
    }

    #[expect(
        clippy::pattern_type_mismatch,
        reason = "Borrowed requests retain submitted payload ownership"
    )]
    /// Applies one validated command to endpoint state.
    fn response_for(
        &mut self,
        request: &Request,
    ) -> io::Result<ResponseEnvelope> {
        if let Some(response) = self.endpoint_response(request)? {
            return Ok(response);
        }
        if let Some(response) = self.share_response(request)? {
            return Ok(response);
        }
        match request {
            Request::Snapshot => {
                Ok(ResponseEnvelope::snapshot(self.sharing.snapshot()))
            }
            Request::Status => Ok(ResponseEnvelope::ready()),
            Request::SubmitFile { path } => {
                let source =
                    OutboundSource::open(path).map_err(io::Error::other)?;
                let attachment = Attachment::file(
                    &source.name().to_string_lossy(),
                    source.len(),
                );
                let share_id = self.queue_attachment(attachment);
                self.outbound.remember_file(share_id, source);
                Ok(ResponseEnvelope::queued(share_id))
            }
            Request::SubmitText { text } => {
                let share_id = self.queue_attachment(Attachment::text(text));
                Ok(ResponseEnvelope::queued(share_id))
            }
            Request::SubmitUrl { url } => {
                let share_id = self.queue_attachment(Attachment::url(url));
                Ok(ResponseEnvelope::queued(share_id))
            }
            Request::SimulateFail { .. }
            | Request::SimulateIncomingFile { .. }
            | Request::SimulateIncomingText { .. }
            | Request::SimulateIncomingUrl { .. }
            | Request::SimulatePeerAccept { .. }
            | Request::SimulatePeerLost { .. }
            | Request::SimulatePeerReject { .. }
            | Request::SimulatePeerSeen { .. }
            | Request::SimulateProgress { .. }
            | _ => Ok(self.simulation_response(request)),
        }
    }

    /// Accepts and queues the next local control request.
    ///
    /// # Errors
    ///
    /// Returns an error when listener configuration or a client fails.
    #[inline]
    pub fn serve_next(&mut self, listener: &UnixListener) -> io::Result<()> {
        self.apply_network_events()?;
        let (mut stream, _address) = listener.accept()?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let request = read_request(&mut reader)?;
        if request.version() != PROTOCOL_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "client uses an unsupported control protocol",
            ));
        }
        let response = self.response_for(request.request())?;
        if matches!(response.response(), Response::Queued { .. }) {
            self.queued.push(request);
        }
        write_response(&mut stream, &response)
    }

    /// Serves control clients until the owning process requests shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error when listener configuration or a client fails.
    #[inline]
    pub fn serve_until<Stopped>(
        &mut self,
        listener: &UnixListener,
        mut stopped: Stopped,
    ) -> io::Result<()>
    where
        Stopped: FnMut() -> bool,
    {
        listener.set_nonblocking(true)?;
        while !stopped() {
            match self.serve_next(listener) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    /// Applies consent and peer-selection actions when they are recognized.
    #[expect(
        clippy::pattern_type_mismatch,
        clippy::wildcard_enum_match_arm,
        reason = "Borrowed non-exhaustive requests require an unhandled case"
    )]
    fn share_response(
        &mut self,
        request: &Request,
    ) -> io::Result<Option<ResponseEnvelope>> {
        let response = match request {
            Request::Accept { share_id } => {
                let accepted = self.sharing.accept_inbound(*share_id);
                if accepted && let Some(network) = &self.network {
                    network.accept_inbound(*share_id)?;
                }
                action_response(accepted)
            }
            Request::Cancel { share_id } => {
                if self.sharing.cancel(*share_id) {
                    if let Some(network) = &self.network {
                        network.cancel_transfer(*share_id);
                    }
                    self.outbound.finish(*share_id);
                    ResponseEnvelope::cancelled()
                } else {
                    ResponseEnvelope::not_found()
                }
            }
            Request::Dismiss { share_id } => {
                action_response(self.sharing.dismiss(*share_id))
            }
            Request::PinPeer { peer_id } => {
                action_response(self.sharing.pin_peer(peer_id))
            }
            Request::Reject { share_id } => {
                let rejected = self.sharing.reject_inbound(*share_id);
                if rejected && let Some(network) = &self.network {
                    network.reject_inbound(*share_id)?;
                }
                action_response(rejected)
            }
            Request::SelectPeer { peer_id, share_id } => {
                action_response(self.select_peer(*share_id, peer_id))
            }
            _ => return Ok(None),
        };
        Ok(Some(response))
    }

    /// Creates an endpoint backed by deterministic local peers.
    #[must_use]
    #[inline]
    pub fn simulated() -> Self {
        let mut endpoint = Self {
            network: None,
            outbound: OutboundState::default(),
            queued: Vec::new(),
            sharing: Coordinator::new(),
            simulated: true,
        };
        endpoint.sharing.observe_peer("pixel-8", "Ollie's Pixel");
        endpoint.sharing.observe_peer("galaxy-tab", "Galaxy Tab");
        endpoint
    }

    /// Applies one simulator request and reports whether state changed.
    #[expect(
        clippy::pattern_type_mismatch,
        clippy::wildcard_enum_match_arm,
        reason = "Borrowed non-exhaustive requests require an unhandled case"
    )]
    fn simulation_applied(&mut self, request: &Request) -> bool {
        match request {
            Request::SimulateDiscoveryTimeout => {
                self.sharing.discovery_timed_out()
            }
            Request::SimulateFail { share_id } => self.sharing.fail(*share_id),
            Request::SimulateIncomingFile { name, size_bytes } => self
                .sharing
                .offer_inbound(Attachment::file(name, *size_bytes), "pixel-8")
                .is_some(),
            Request::SimulateIncomingText { text } => self
                .sharing
                .offer_inbound(Attachment::text(text), "pixel-8")
                .is_some(),
            Request::SimulateIncomingUrl { url } => self
                .sharing
                .offer_inbound(Attachment::url(url), "pixel-8")
                .is_some(),
            Request::SimulatePeerAccept { share_id } => {
                self.sharing.accept_by_peer(*share_id)
            }
            Request::SimulatePeerLost { peer_id } => {
                self.sharing.remove_peer(peer_id)
            }
            Request::SimulatePeerReject { share_id } => {
                self.sharing.reject_by_peer(*share_id)
            }
            Request::SimulatePeerSeen { name, peer_id } => {
                self.sharing.observe_peer(peer_id, name);
                true
            }
            Request::SimulateProgress {
                share_id,
                transferred_bytes,
            } => self.sharing.record_progress(*share_id, *transferred_bytes),
            _ => false,
        }
    }

    /// Applies a deterministic peer event when simulation is enabled.
    fn simulation_response(&mut self, request: &Request) -> ResponseEnvelope {
        if !self.simulated {
            return ResponseEnvelope::not_found();
        }
        action_response(self.simulation_applied(request))
    }

    /// Attaches the production network worker to an otherwise empty daemon.
    #[expect(
        clippy::single_call_fn,
        reason = "Only production startup attaches the real worker"
    )]
    fn with_network_worker(network: NetworkWorker) -> Self {
        Self {
            network: Some(network),
            ..Self::new()
        }
    }
}

/// Converts a state transition result into a control response.
const fn action_response(applied: bool) -> ResponseEnvelope {
    if applied {
        ResponseEnvelope::applied()
    } else {
        ResponseEnvelope::not_found()
    }
}

/// Builds public file facts after confirming the source still exists.
/// Removes an abandoned socket without replacing a running endpoint.
#[expect(
    clippy::single_call_fn,
    reason = "The stale-socket decision remains separate from binding"
)]
fn remove_stale_socket(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    match UnixStream::connect(path) {
        Ok(_stream) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            "local endpoint is already running",
        )),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound
            ) =>
        {
            fs::remove_file(path)
        }
        Err(error) => Err(error),
    }
}

/// Runs the local endpoint until the process is terminated.
///
/// # Errors
///
/// Returns an error when the private control socket cannot be served.
#[inline]
pub fn run(socket_path: &Path) -> io::Result<()> {
    let socket = ControlSocket::bind(socket_path)?;
    let network = NetworkWorker::start()?;
    Daemon::with_network_worker(network).serve_until(&socket.listener, || false)
}

/// Runs a deterministic local peer for complete application testing.
///
/// # Errors
///
/// Returns an error when the private control socket cannot be served.
#[inline]
pub fn run_simulated(socket_path: &Path) -> io::Result<()> {
    let socket = ControlSocket::bind(socket_path)?;
    Daemon::simulated().serve_until(&socket.listener, || false)
}
