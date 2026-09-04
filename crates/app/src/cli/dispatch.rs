//! Dispatches classified requests against the local endpoint.

use std::env;
use std::io;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use clap::Parser;
use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_response, write_request, write_response};
use quickshare_control::request::Envelope as RequestEnvelope;
use quickshare_control::response::{Envelope as ResponseEnvelope, Response};
use quickshare_sharing::{Direction, EndpointSnapshot, Phase};

use super::args::{
    Cli, Command, ConfigCommand, DiscoverAction, PeerCommand, ShareCommand,
    SimulateCommand, VisibilityAction, parse,
};
use super::classify::request;
use super::log;
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
                 with omarchy-quickshare daemon",
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
    match parse(arguments.iter().cloned()) {
        Ok(cli) => dispatch(cli, current_directory, socket_path, output),
        Err(error) => clap_result(error, output),
    }
}

/// Applies a parsed command without contacting the daemon for help or version.
fn dispatch<Output>(
    cli: Cli,
    current_directory: &Path,
    socket_path: &Path,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    match cli.into_command() {
        Command::Daemon {
            simulate,
            log_level,
        } => {
            log::init(log_level);
            tracing::info!(simulate, "starting daemon");
            if simulate {
                daemon::run_simulated(socket_path)
            } else {
                daemon::run(socket_path)
            }
        }
        command => execute(command, current_directory, socket_path, output),
    }
}

fn clap_result<Output>(
    error: clap::Error,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    if error.use_stderr() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, error));
    }
    write!(output, "{error}")
}

fn execute<Output>(
    command: Command,
    current_directory: &Path,
    socket_path: &Path,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    match command {
        Command::Config { action } => config_command(action, output),
        Command::Daemon { .. } => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "daemon commands start locally",
        )),
        Command::Discover { action } => write_action(
            output,
            exchange(socket_path, &discover_request(action))?.response(),
        ),
        Command::Health => {
            let response = exchange(socket_path, &RequestEnvelope::status())?;
            if matches!(response.response(), Response::Ready) {
                return writeln!(output, "available");
            }
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "endpoint did not confirm readiness",
            ))
        }
        Command::Peer { action } => write_action(
            output,
            exchange(socket_path, &peer_request(&action))?.response(),
        ),
        Command::ProtocolVersion => writeln!(output, "{PROTOCOL_VERSION}"),
        Command::Send { content } => {
            let request = request(&content, current_directory)?;
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
        Command::Share { action } => write_action(
            output,
            exchange(socket_path, &share_request(&action))?.response(),
        ),
        Command::Simulate { action } => write_action(
            output,
            exchange(socket_path, &simulate_request(&action)?)?.response(),
        ),
        Command::Status { json } => {
            let response = exchange(socket_path, &RequestEnvelope::snapshot())?;
            if json {
                write_response(output, &response)
            } else {
                write_status(output, response.response())
            }
        }
        Command::Visibility { action } => write_action(
            output,
            exchange(socket_path, &visibility_request(action))?.response(),
        ),
    }
}

fn config_command<Output>(
    action: ConfigCommand,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    match action {
        ConfigCommand::Set { key, value } => {
            let mut config = Config::load()?;
            config.set(&key, &value)?;
            writeln!(output, "Config updated.")
        }
        ConfigCommand::Show => write!(output, "{}", Config::load()?.to_toml()),
    }
}

const fn discover_request(action: DiscoverAction) -> RequestEnvelope {
    match action {
        DiscoverAction::Start => RequestEnvelope::discover(),
        DiscoverAction::Stop => RequestEnvelope::stop_discovery(),
    }
}

fn peer_request(action: &PeerCommand) -> RequestEnvelope {
    match action {
        PeerCommand::Pin { peer_id } => RequestEnvelope::pin_peer(peer_id),
        PeerCommand::Unpin => RequestEnvelope::unpin_peer(),
    }
}

fn share_request(action: &ShareCommand) -> RequestEnvelope {
    match action {
        ShareCommand::Accept { share_id } => RequestEnvelope::accept(*share_id),
        ShareCommand::Cancel { share_id } => RequestEnvelope::cancel(*share_id),
        ShareCommand::Dismiss { share_id } => {
            RequestEnvelope::dismiss(*share_id)
        }
        ShareCommand::Reject { share_id } => RequestEnvelope::reject(*share_id),
        ShareCommand::Select { share_id, peer_id } => {
            RequestEnvelope::select_peer(*share_id, peer_id)
        }
    }
}

fn visibility_request(action: VisibilityAction) -> RequestEnvelope {
    match action {
        VisibilityAction::Close => RequestEnvelope::close_visibility(),
        VisibilityAction::Open => RequestEnvelope::open_visibility(),
    }
}

fn simulate_request(action: &SimulateCommand) -> io::Result<RequestEnvelope> {
    if env::var_os("OMARCHY_QUICKSHARE_ALLOW_SIMULATION").is_none() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "simulation commands are unavailable; start a simulated daemon \
             with daemon --simulate and set \
             OMARCHY_QUICKSHARE_ALLOW_SIMULATION=1",
        ));
    }
    Ok(match action {
        SimulateCommand::DiscoveryTimeout => {
            RequestEnvelope::simulate_discovery_timeout()
        }
        SimulateCommand::Fail { share_id } => {
            RequestEnvelope::simulate_fail(*share_id)
        }
        SimulateCommand::IncomingFile { name, size_bytes } => {
            RequestEnvelope::simulate_incoming_file(name, *size_bytes)
        }
        SimulateCommand::IncomingText { text } => {
            RequestEnvelope::simulate_incoming_text(text)
        }
        SimulateCommand::IncomingUrl { url } => {
            RequestEnvelope::simulate_incoming_url(url)
        }
        SimulateCommand::PeerAccept { share_id } => {
            RequestEnvelope::simulate_peer_accept(*share_id)
        }
        SimulateCommand::PeerLost { peer_id } => {
            RequestEnvelope::simulate_peer_lost(peer_id)
        }
        SimulateCommand::PeerReject { share_id } => {
            RequestEnvelope::simulate_peer_reject(*share_id)
        }
        SimulateCommand::PeerSeen { peer_id, name } => {
            RequestEnvelope::simulate_peer_seen(peer_id, name)
        }
        SimulateCommand::Progress {
            share_id,
            transferred_bytes,
        } => RequestEnvelope::simulate_progress(*share_id, *transferred_bytes),
    })
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
             status --json and retry",
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

fn socket_path() -> io::Result<PathBuf> {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|root| root.join("omarchy-quickshare/control.sock"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not available",
            )
        })
}

/// Runs one command using process arguments and the user runtime directory.
///
/// # Errors
///
/// Returns an error when arguments, environment, or local control are invalid.
#[inline]
pub fn run_from_environment() -> io::Result<()> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let current_directory = env::current_dir()?;
    let socket_path = socket_path()?;
    let stdout = io::stdout();
    dispatch(cli, &current_directory, &socket_path, &mut stdout.lock())
}
