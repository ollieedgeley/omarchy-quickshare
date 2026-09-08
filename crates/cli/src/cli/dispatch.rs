//! Dispatches classified requests against the local endpoint.

use std::env;
use std::io;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::client::{
    exchange, exchange_stream, generate_request_id,
};
use quickshare_control::codec::write_response;
use quickshare_control::request::Envelope as RequestEnvelope;
use quickshare_control::response::{ErrorCode, Response};

use super::args::{
    Command, ConfigCommand, DiscoverAction, PeerCommand, ShareCommand,
    SimulateCommand, VisibilityAction, parse,
};
use super::classify::{request, request_for_peer};
use super::snapshot_output::{write_operation, write_status};
use quickshare_config::Config;

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
        Ok(cli) => {
            execute(cli.into_command(), current_directory, socket_path, output)
        }
        Err(error) => clap_result(error, output),
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
        Command::Config { action } => {
            config_command(action, Some(socket_path), output)
        }
        Command::Discover { action } => write_action(
            output,
            exchange(socket_path, &discover_request(action))?.response(),
        ),
        Command::Health => {
            let response = exchange(socket_path, &RequestEnvelope::status())?;
            if matches!(response.response(), Response::Ready) {
                return writeln!(output, "available");
            }
            Err(unexpected_response(
                response.response(),
                "endpoint did not confirm readiness",
            ))
        }
        Command::Peer { action } => write_action(
            output,
            exchange(socket_path, &peer_request(&action))?.response(),
        ),
        Command::ProtocolVersion => writeln!(output, "{PROTOCOL_VERSION}"),
        Command::Send {
            clipboard,
            content,
            peer,
        } => {
            let current_directory = (!clipboard).then_some(current_directory);
            let request_id = generate_request_id()?;
            let request = match peer.as_deref() {
                Some(peer_id) => request_for_peer(
                    &content,
                    current_directory,
                    peer_id,
                    &request_id,
                )?,
                None => request(&content, current_directory, &request_id)?,
            };
            let response = exchange(socket_path, &request)
                .map_err(|error| uncertain_submission(&request_id, &error))?;
            if let Response::Queued { operation } = response.response() {
                writeln!(output, "Share {} queued.", operation.id_string)?;
                writeln!(output, "request_id={request_id}")
            } else {
                if matches!(
                    response.response(),
                    Response::Error {
                        code: ErrorCode::OutcomeUnknown
                            | ErrorCode::RequestTimeout
                            | ErrorCode::PersistenceFailed
                    }
                ) {
                    return Err(uncertain_submission(
                        &request_id,
                        &unexpected_response(
                            response.response(),
                            "acceptance unknown",
                        ),
                    ));
                }
                Err(unexpected_response(
                    response.response(),
                    "endpoint did not queue the share",
                ))
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
        Command::Operation { request_id, json } => {
            let response = exchange(
                socket_path,
                &RequestEnvelope::operation(&request_id),
            )?;
            match response.response() {
                Response::Operation { operation } => {
                    if json {
                        write_response(output, &response)
                    } else {
                        write_operation(output, operation)
                    }
                }
                Response::NotFound
                | Response::Error {
                    code: ErrorCode::OutcomeUnknown,
                } => {
                    if json {
                        write_response(output, &response)?;
                    }
                    Err(io::Error::other(format!(
                        "Operation {request_id} is unknown or expired; \
                         this does not prove it was never accepted. \
                         Do not blindly resubmit."
                    )))
                }
                _ => Err(unexpected_response(
                    response.response(),
                    "endpoint did not return an operation",
                )),
            }
        }
        Command::Status { json } => {
            let response = exchange(socket_path, &RequestEnvelope::snapshot())?;
            if matches!(
                response.response(),
                Response::Error {
                    code: ErrorCode::MessageTooLarge
                }
            ) {
                return Err(io::Error::other(
                    "Full snapshot exceeds the control message limit. \
                     Use omarchy-quickshare operation <request_id> --json \
                     with the retained request ID for a compact operation \
                     outcome. Do not blindly resubmit.",
                ));
            }
            if !matches!(response.response(), Response::Snapshot { .. }) {
                return Err(unexpected_response(
                    response.response(),
                    "endpoint did not return a snapshot",
                ));
            }
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
    socket_path: Option<&Path>,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    match action {
        ConfigCommand::Set { key, value } => {
            match socket_path.map_or_else(
                || Err(io::Error::from(io::ErrorKind::NotFound)),
                UnixStream::connect,
            ) {
                Ok(stream) => {
                    let response = exchange_stream(
                        stream,
                        &RequestEnvelope::patch_preferences(&key, &value),
                    )?;
                    write_action(output, response.response())
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound
                            | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    let _saved = Config::patch(&key, &value)?;
                    writeln!(
                        output,
                        "Preferences saved; daemon offline, not applied."
                    )
                }
                Err(error) => Err(error),
            }
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
             with omarchy-quickshare-daemon --simulate and set \
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
        SimulateCommand::Complete { share_id } => {
            RequestEnvelope::simulate_complete(*share_id)
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
        Response::Preferences { preferences } => {
            if let Some(error) = &preferences.error {
                return Err(io::Error::other(error.clone()));
            }
            if preferences.pending {
                writeln!(output, "Preferences saved; activation pending.")
            } else {
                writeln!(output, "Preferences saved and applied.")
            }
        }
        Response::Cancelled => writeln!(output, "Share cancelled."),
        Response::NotFound => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "action is not available in the current state; check \
             status --json and retry",
        )),
        _ => Err(unexpected_response(
            response,
            "endpoint returned an unsupported response",
        )),
    }
}

fn uncertain_submission(request_id: &str, error: &io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "{error}; submission acceptance is unknown. \
             Retained request ID: {request_id}. Reconcile with: \
             omarchy-quickshare operation {request_id} --json. \
             Do not blindly resubmit."
        ),
    )
}

fn unexpected_response(response: &Response, fallback: &str) -> io::Error {
    let message = match response {
        Response::Error { code } => match code {
            ErrorCode::InvalidRequest => "endpoint rejected an invalid request",
            ErrorCode::UnsupportedVersion => {
                "endpoint uses an unsupported control protocol; \
                 reinstall the matching omarchy-quickshare package"
            }
            ErrorCode::MessageTooLarge => {
                "control message exceeds the endpoint size limit"
            }
            ErrorCode::RequestTimeout => {
                "endpoint timed out processing the control request"
            }
            ErrorCode::Conflict => {
                "request ID conflicts with a different submission"
            }
            ErrorCode::Busy => "endpoint operation capacity is full",
            ErrorCode::PersistenceFailed => {
                "endpoint could not persist the operation"
            }
            ErrorCode::OutcomeUnknown => {
                "operation outcome is unknown; do not blindly resubmit"
            }
            _ => "endpoint rejected the control request",
        },
        Response::NotFound => {
            "action is not available in the current state; \
             check status --json and retry"
        }
        _ => fallback,
    };
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
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
    let cli = match parse(env::args_os().skip(1)) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let command = cli.into_command();
    let stdout = io::stdout();
    if matches!(command, Command::ProtocolVersion) {
        return writeln!(stdout.lock(), "{PROTOCOL_VERSION}");
    }
    if let Command::Config { action } = command {
        let socket_path = socket_path().ok();
        return config_command(
            action,
            socket_path.as_deref(),
            &mut stdout.lock(),
        );
    }
    let current_directory = env::current_dir()?;
    let socket_path = socket_path()?;
    execute(
        command,
        &current_directory,
        &socket_path,
        &mut stdout.lock(),
    )
}
