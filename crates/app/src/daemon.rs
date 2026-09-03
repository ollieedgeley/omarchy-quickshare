//! Local endpoint lifecycle and outbound queue ownership.

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

/// Owner-only mode for the control socket directory.
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
/// Owner-only mode for the control socket.
const PRIVATE_SOCKET_MODE: u32 = 0o600;

/// The same-user local endpoint state.
#[derive(Debug, Default)]
pub struct Daemon {
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

impl Daemon {
    /// Creates an empty local endpoint.
    #[must_use]
    #[inline]
    pub const fn new() -> Self {
        Self {
            queued: Vec::new(),
            sharing: Coordinator::new(),
            simulated: false,
        }
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
        match request {
            Request::Accept { share_id } => {
                Ok(action_response(self.sharing.accept_inbound(*share_id)))
            }
            Request::Cancel { share_id } => {
                Ok(if self.sharing.cancel(*share_id) {
                    ResponseEnvelope::cancelled()
                } else {
                    ResponseEnvelope::not_found()
                })
            }
            Request::Reject { share_id } => {
                Ok(action_response(self.sharing.reject_inbound(*share_id)))
            }
            Request::SelectPeer { peer_id, share_id } => Ok(action_response(
                self.sharing.select_peer(*share_id, peer_id),
            )),
            Request::Snapshot => {
                Ok(ResponseEnvelope::snapshot(self.sharing.snapshot()))
            }
            Request::Status => Ok(ResponseEnvelope::ready()),
            Request::SubmitFile { path } => {
                let attachment = file_attachment(path)?;
                let _share_id = self.sharing.queue_outbound(attachment);
                Ok(ResponseEnvelope::queued())
            }
            Request::SubmitText { text } => {
                let _share_id =
                    self.sharing.queue_outbound(Attachment::text(text));
                Ok(ResponseEnvelope::queued())
            }
            Request::SubmitUrl { url } => {
                let _share_id =
                    self.sharing.queue_outbound(Attachment::url(url));
                Ok(ResponseEnvelope::queued())
            }
            Request::SimulateIncomingText { .. }
            | Request::SimulatePeerAccept { .. }
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
        if matches!(response.response(), Response::Queued) {
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

    /// Creates an endpoint backed by deterministic local peers.
    #[must_use]
    #[inline]
    pub fn simulated() -> Self {
        let mut endpoint = Self {
            queued: Vec::new(),
            sharing: Coordinator::new(),
            simulated: true,
        };
        endpoint.sharing.observe_peer("pixel-8", "Ollie's Pixel");
        endpoint.sharing.observe_peer("galaxy-tab", "Galaxy Tab");
        endpoint
    }

    /// Applies a deterministic peer event when simulation is enabled.
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "Borrowed simulation records retain their payload ownership"
    )]
    fn simulation_response(&mut self, request: &Request) -> ResponseEnvelope {
        if !self.simulated {
            return ResponseEnvelope::not_found();
        }
        let applied = match request {
            Request::SimulateIncomingText { text } => self
                .sharing
                .offer_inbound(Attachment::text(text), "pixel-8")
                .is_some(),
            Request::SimulatePeerAccept { share_id } => {
                self.sharing.accept_by_peer(*share_id)
            }
            Request::SimulateProgress {
                share_id,
                transferred_bytes,
            } => self.sharing.record_progress(*share_id, *transferred_bytes),
            Request::Accept { .. }
            | Request::Cancel { .. }
            | Request::Reject { .. }
            | Request::SelectPeer { .. }
            | Request::Snapshot
            | Request::Status
            | Request::SubmitFile { .. }
            | Request::SubmitText { .. }
            | Request::SubmitUrl { .. }
            | _ => false,
        };
        action_response(applied)
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
#[expect(
    clippy::single_call_fn,
    reason = "File metadata validation stays outside control dispatch"
)]
fn file_attachment(path: &Path) -> io::Result<Attachment> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "file path has no name")
    })?;
    let size_bytes = fs::metadata(path)?.len();
    Ok(Attachment::file(&name.to_string_lossy(), size_bytes))
}

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
    Daemon::new().serve_until(&socket.listener, || false)
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
