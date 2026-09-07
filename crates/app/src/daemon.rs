//! Local endpoint lifecycle and outbound queue ownership.

mod lifecycle;
mod media;
mod network;
mod notify;
mod observations;
mod outbound;
mod preferences;
mod production;
mod visibility;

pub use self::lifecycle::{run, run_simulated};

use core::time::Duration;
use std::io::{self, BufReader};
use std::os::unix::net::UnixListener;
use std::thread;

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_request, write_response};
use quickshare_control::request::{Envelope as RequestEnvelope, Request};
use quickshare_control::response::{
    Envelope as ResponseEnvelope, PreferenceStatus, Response,
};
use quickshare_sharing::{Attachment, Coordinator};

use self::network::NetworkWorker;
use self::outbound::OutboundState;

/// The same-user local endpoint state.
#[derive(Debug, Default)]
pub struct Daemon {
    /// Persisted user settings applied to this endpoint.
    config: crate::config::Config,
    /// Parent-directory notifications for live user edits.
    config_watch: Option<preferences::ConfigWatch>,
    /// When the current outbound search started.
    discovery_started_at: Option<std::time::Instant>,
    /// Production network worker, omitted by in-process and simulated daemons.
    network: Option<NetworkWorker>,
    /// Production-only file paths and discovered LAN routes.
    outbound: OutboundState,
    /// Durable, active, and failed preference outcomes.
    preferences: PreferenceStatus,
    /// Outbound shares accepted from local clients.
    queued: Vec<RequestEnvelope>,
    /// User-visible share lifecycle state.
    sharing: Coordinator,
    /// Whether deterministic peer events are accepted.
    simulated: bool,
    /// Durations captured by each admitted timed operation.
    timeouts: lifecycle::OperationTimeouts,
    /// When the active share entered the transferring phase.
    transfer_started_at: Option<std::time::Instant>,
    /// Shared permission, listener leases, and suspend-inclusive expiry.
    visibility: visibility::Visibility,
    /// Admitted inbound consent retained independently of visibility.
    pending_consent: Option<alloc::sync::Arc<visibility::PendingConsent>>,
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
            Request::CloseVisibility => self.visibility.close(),
            Request::Discover => {
                self.sharing.start_discovery();
                tracing::info!(
                    target: "omarchy_quickshare::protocol",
                    stage = "local_control",
                    operation = "discover",
                    outcome = "completed",
                    phase = "searching",
                    "discovery started"
                );
                if let Some(network) = &self.network {
                    network.discover()?;
                }
            }
            Request::OpenVisibility => {
                if let Some(generation) = self.visibility.request_open() {
                    self.activate_visibility(generation);
                }
            }
            Request::StopDiscovery => {
                self.sharing.stop_discovery();
                tracing::info!(
                    target: "omarchy_quickshare::protocol",
                    stage = "local_control",
                    operation = "stop_discovery",
                    outcome = "completed",
                    phase = "idle",
                    "discovery stopped"
                );
                if let Some(network) = &self.network {
                    network.stop_discovery()?;
                }
            }
            _ => return Ok(None),
        }
        self.sync_visibility();
        Ok(Some(ResponseEnvelope::applied()))
    }

    /// Creates an empty local endpoint.
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self {
            config: crate::config::Config::default(),
            config_watch: None,
            discovery_started_at: None,
            network: None,
            outbound: OutboundState::default(),
            preferences: PreferenceStatus::default(),
            queued: Vec::new(),
            sharing: Coordinator::new(),
            simulated: false,
            timeouts: lifecycle::OperationTimeouts::default(),
            transfer_started_at: None,
            visibility: visibility::Visibility::default(),
            pending_consent: None,
        }
    }

    #[cfg(test)]
    /// Queues one validated attachment and reports its stable identifier.
    fn queue_attachment(&mut self, attachment: Attachment) -> u64 {
        self.queue_attachment_for(attachment, false)
    }

    fn queue_attachment_for(
        &mut self,
        attachment: Attachment,
        ignore_pin: bool,
    ) -> u64 {
        let share_id = if ignore_pin {
            self.sharing.queue_outbound_unpinned(attachment)
        } else {
            self.sharing.queue_outbound(attachment)
        }
        .get();
        tracing::info!(
            share_id,
            direction = "outbound",
            phase = "waiting_for_peer",
            "share phase"
        );
        self.sharing.start_discovery();
        tracing::info!(phase = "searching", "discovery started");
        if let Some(network) = &self.network
            && network.discover().is_err()
        {
            tracing::error!(stage = "discover", "daemon cannot continue");
        }
        share_id
    }
    fn queued_response(
        &mut self,
        share_id: u64,
        peer_id: Option<&str>,
    ) -> ResponseEnvelope {
        let selected = if let Some(peer_id) = peer_id {
            self.select_peer(share_id, peer_id)
        } else {
            let _started = self.start_pinned_outbound(share_id);
            true
        };
        if selected {
            return ResponseEnvelope::queued(share_id);
        }
        let _cancelled = self.sharing.cancel(share_id);
        self.outbound.finish(share_id);
        ResponseEnvelope::not_found()
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
        self.apply_timeouts()?;
        if let Some(response) = self.endpoint_response(request)? {
            return Ok(response);
        }
        if let Some(response) = self.share_response(request)? {
            return Ok(response);
        }
        match request {
            Request::PatchPreferences { key, value } => {
                Ok(self.patch_preferences(key, value))
            }
            Request::Snapshot => Ok(ResponseEnvelope::snapshot(
                self.sharing.snapshot(),
                &self.preferences,
            )),
            Request::Status => Ok(ResponseEnvelope::ready()),
            Request::SubmitFile { .. }
            | Request::SubmitText { .. }
            | Request::SubmitUrl { .. }
                if self.has_active_share() =>
            {
                Ok(ResponseEnvelope::not_found())
            }
            Request::SubmitFile { path, peer_id } => {
                let share_id = self.queue_file(path, peer_id.as_deref())?;
                Ok(self.queued_response(share_id, peer_id.as_deref()))
            }
            Request::SubmitText { peer_id, text } => {
                let share_id = self.queue_attachment_for(
                    Attachment::text(text),
                    peer_id.is_some(),
                );
                self.outbound.remember_text(share_id, text.clone());
                Ok(self.queued_response(share_id, peer_id.as_deref()))
            }
            Request::SubmitUrl { peer_id, url } => {
                let share_id = self.queue_attachment_for(
                    Attachment::url(url),
                    peer_id.is_some(),
                );
                self.outbound.remember_url(share_id, url.clone());
                Ok(self.queued_response(share_id, peer_id.as_deref()))
            }
            Request::SimulateFail { .. }
            | Request::SimulateIncomingFile { .. }
            | Request::SimulateIncomingText { .. }
            | Request::SimulateIncomingUrl { .. }
            | Request::SimulatePeerAccept { .. }
            | Request::SimulatePeerLost { .. }
            | Request::SimulatePeerReject { .. }
            | Request::SimulatePeerSeen { .. }
            | Request::SimulateComplete { .. }
            | Request::SimulateProgress { .. }
            | _ => Ok(self.simulation_response(request)),
        }
    }

    /// Accepts and queues the next local control request.
    ///
    /// # Errors
    ///
    /// Returns an error when the listener or endpoint request handling fails.
    #[inline]
    pub fn serve_next(&mut self, listener: &UnixListener) -> io::Result<()> {
        self.apply_network_events()?;
        self.apply_timeouts()?;
        let (mut stream, _address) = listener.accept()?;
        let Ok(request) = read_request(&mut BufReader::new(&mut stream)) else {
            return Ok(());
        };
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
        // A disconnected client must not stop service for other clients.
        let _delivery_result = write_response(&mut stream, &response);
        Ok(())
    }

    /// Serves control clients until the owning process requests shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error when the listener or endpoint request handling fails.
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
            self.apply_timeouts()?;
            self.reload_preferences()?;
            match self.serve_next(listener) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    tracing::error!(
                        error_class = "io",
                        "daemon cannot continue"
                    );
                    return Err(error);
                }
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
                let accepted = self.accept_pending(*share_id);
                if accepted {
                    tracing::info!(
                        target: "omarchy_quickshare::protocol",
                        stage = "local_control",
                        operation = "accept",
                        outcome = "completed",
                        share_id,
                        direction = "inbound",
                        phase = "transferring",
                        "share phase"
                    );
                    self.transfer_started_at = Some(std::time::Instant::now());
                }
                action_response(accepted)
            }
            Request::Cancel { share_id } => {
                if self.sharing.cancel(*share_id) {
                    if let Some(pending) = self.pending_consent.as_ref()
                        && pending.share_id() == Some(*share_id)
                    {
                        pending.cancel();
                    }
                    tracing::info!(
                        target: "omarchy_quickshare::protocol",
                        stage = "local_control",
                        operation = "cancel",
                        outcome = "completed",
                        share_id,
                        phase = "cancelled",
                        "share phase"
                    );
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
                let dismissed = self.sharing.dismiss(*share_id);
                if dismissed {
                    self.outbound.finish(*share_id);
                }
                action_response(dismissed)
            }
            Request::PinPeer { peer_id } => {
                if self
                    .sharing
                    .snapshot()
                    .peers()
                    .iter()
                    .any(|peer| peer.id() == peer_id)
                {
                    self.patch_preferences("pinned_peer_id", peer_id)
                } else {
                    ResponseEnvelope::not_found()
                }
            }
            Request::UnpinPeer => self.patch_preferences("pinned_peer_id", ""),
            Request::Reject { share_id } => {
                let rejected = self.reject_pending(*share_id);
                if rejected {
                    tracing::info!(
                        target: "omarchy_quickshare::protocol",
                        stage = "local_control",
                        operation = "reject",
                        outcome = "completed",
                        share_id,
                        direction = "inbound",
                        phase = "rejected",
                        "share phase"
                    );
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
            simulated: true,
            ..Self::new()
        };
        endpoint.sharing.observe_peer("pixel-8", "Ollie's Pixel");
        endpoint.sharing.observe_peer("galaxy-tab", "Galaxy Tab");
        endpoint
    }

    /// Applies a deterministic peer event when simulation is enabled.
    fn simulation_response(&mut self, request: &Request) -> ResponseEnvelope {
        if !self.simulated {
            return ResponseEnvelope::not_found();
        }
        action_response(self.simulation_applied(request))
    }

    /// Attaches the production network worker to an otherwise empty daemon.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "Only production startup attaches the real worker"
        )
    )]
    fn with_network_worker(network: NetworkWorker) -> Self {
        Self {
            visibility: network.visibility(),
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
