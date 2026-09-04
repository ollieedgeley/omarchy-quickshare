//! Dispatches classified requests against the local endpoint.

use std::env;
use std::io;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_response, write_request, write_response};
use quickshare_control::request::Envelope as RequestEnvelope;
use quickshare_control::response::{Envelope as ResponseEnvelope, Response};
use quickshare_sharing::{Direction, EndpointSnapshot, Phase};

use super::classify::{action_request, request};
use crate::config::Config;
use crate::daemon;

/// Exchanges one validated request with the local endpoint.
fn exchange(
    socket_path: &Path,
    request: &RequestEnvelope,
) -> io::Result<ResponseEnvelope> {
    let mut stream = UnixStream::connect(socket_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "cannot reach the local endpoint at {}: {error}; start it \
                 with omarchy-quickshare --daemon",
                socket_path.display()
            ),
        )
    })?;
    write_request(&mut stream, request)?;
    let response = read_response(&mut BufReader::new(stream))?;
    if response.version() != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint uses an unsupported control protocol; reinstall the \
             matching omarchy-quickshare package",
        ));
    }
    Ok(response)
}
/// Runs one command against the local endpoint.
///
/// # Errors
///
/// Returns an I/O error when local control cannot complete the command.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed responses remain available for validation"
)]
#[inline]
pub fn run<Output>(
    arguments: &[String],
    current_directory: &Path,
    socket_path: &Path,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    if matches!(arguments, [argument] if argument == "--protocol-version") {
        return writeln!(output, "{PROTOCOL_VERSION}");
    }
    if matches!(arguments, [argument] if argument == "--config") {
        return write!(output, "{}", Config::load()?.to_toml());
    }
    if let [flag, key, value] = arguments
        && flag == "--config-set"
    {
        let mut config = Config::load()?;
        config.set(key, value)?;
        return writeln!(output, "Config updated.");
    }
    if matches!(arguments, [argument] if argument == "--runtime-status") {
        let response = exchange(socket_path, &RequestEnvelope::status())?;
        if matches!(response.response(), Response::Ready) {
            return writeln!(output, "available");
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint did not confirm readiness",
        ));
    }
    if matches!(arguments, [argument] if argument == "--status-json") {
        let response = exchange(socket_path, &RequestEnvelope::snapshot())?;
        return write_response(output, &response);
    }
    if matches!(arguments, [argument] if argument == "--status") {
        let response = exchange(socket_path, &RequestEnvelope::snapshot())?;
        return write_status(output, response.response());
    }
    if let Some(action) = action_request(arguments)? {
        let response = exchange(socket_path, &action)?;
        return write_action(output, response.response());
    }
    let request = request(arguments, current_directory)?;
    let response = exchange(socket_path, &request)?;
    match response.response() {
        Response::Queued { share_id } => {
            writeln!(output, "Share {share_id} queued.")
        }
        Response::Applied
        | Response::Cancelled
        | Response::NotFound
        | Response::Ready
        | Response::Snapshot { .. }
        | _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint returned an unsupported response",
        )),
    }
}

/// Writes the user-facing result of a state-changing command.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed responses remain available after rendering"
)]
fn write_action<Output>(
    output: &mut Output,
    response: &Response,
) -> io::Result<()>
where
    Output: Write,
{
    match response {
        Response::Applied => writeln!(output, "Action applied."),
        Response::Cancelled => writeln!(output, "Share cancelled."),
        Response::NotFound => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "action is not available in the current state; check \
             --status-json and retry",
        )),
        Response::Queued { .. }
        | Response::Ready
        | Response::Snapshot { .. }
        | _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint returned an unsupported response",
        )),
    }
}

/// Writes machine-readable endpoint observations as key=value lines.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed snapshot fields are rendered without taking ownership"
)]
fn write_status<Output>(
    output: &mut Output,
    response: &Response,
) -> io::Result<()>
where
    Output: Write,
{
    let Response::Snapshot { snapshot } = response else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint did not return a snapshot",
        ));
    };
    write_snapshot(output, snapshot)
}

/// Renders public snapshot fields used by humans and scripts.
fn write_snapshot<Output>(
    output: &mut Output,
    snapshot: &EndpointSnapshot,
) -> io::Result<()>
where
    Output: Write,
{
    let config = Config::load()?;
    writeln!(
        output,
        "receive_directory={}",
        config.receive_directory.display()
    )?;
    writeln!(output, "discovery={:?}", snapshot.discovery())?;
    writeln!(output, "visibility={:?}", snapshot.visibility())?;
    if let Some(peer) = snapshot.peers().iter().find(|peer| peer.is_pinned()) {
        writeln!(output, "pinned_peer={}", peer.id())?;
    } else if let Some(peer_id) = config.pinned_peer_id.as_deref() {
        writeln!(output, "pinned_peer={peer_id}")?;
    } else {
        writeln!(output, "pinned_peer=")?;
    }
    let Some(active) = snapshot.active_share() else {
        return Ok(());
    };
    writeln!(output, "share_id={}", active.id().get())?;
    writeln!(
        output,
        "direction={}",
        match active.direction() {
            Direction::Inbound => "inbound",
            Direction::Outbound => "outbound",
            _ => "unknown",
        }
    )?;
    writeln!(output, "phase={:?}", active.phase())?;
    writeln!(output, "transferred_bytes={}", active.transferred_bytes())?;
    writeln!(output, "total_bytes={}", active.total_bytes())?;
    if let Some(code) = active.verification_code() {
        writeln!(output, "verification_code={code}")?;
    }
    if let Some((reason, recovery)) = terminal_guidance(active.phase()) {
        writeln!(output, "terminal_reason={reason}")?;
        writeln!(output, "recovery_guidance={recovery}")?;
    }
    Ok(())
}

/// Returns a user-facing reason and recovery hint for a terminal phase.
const fn terminal_guidance(
    phase: Phase,
) -> Option<(&'static str, &'static str)> {
    match phase {
        Phase::Cancelled => Some((
            "share cancelled",
            "Submit the content again if you still want to send it.",
        )),
        Phase::Failed => Some((
            "transfer failed",
            "Retry the share. Confirm the peer is nearby and the receive \
             directory is writable.",
        )),
        Phase::Rejected => Some((
            "share rejected",
            "Choose another peer or ask the receiver to accept.",
        )),
        _ => None,
    }
}

/// Runs one command using process arguments and the user runtime directory.
///
/// # Errors
///
/// Returns an error when arguments, environment, or local control are invalid.
#[inline]
pub fn run_from_environment() -> io::Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let current_directory = env::current_dir()?;
    let socket_path = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|root| root.join("omarchy-quickshare/control.sock"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not available",
            )
        })?;
    if arguments.as_slice() == ["--daemon", "--simulate"] {
        return daemon::run_simulated(&socket_path);
    }
    if matches!(arguments.as_slice(), [argument] if argument == "--daemon") {
        return daemon::run(&socket_path);
    }
    let stdout = io::stdout();
    run(
        &arguments,
        &current_directory,
        &socket_path,
        &mut stdout.lock(),
    )
}
