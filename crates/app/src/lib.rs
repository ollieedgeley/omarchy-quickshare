//! Command-line and daemon composition for Omarchy Quick Share.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream command use"
    )
)]

/// Local endpoint lifecycle and queue ownership.
pub mod daemon;

use std::env;
use std::io;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_response, write_request, write_response};
use quickshare_control::request::Envelope as RequestEnvelope;
use quickshare_control::response::{Envelope as ResponseEnvelope, Response};

/// Exchanges one validated request with the local endpoint.
fn exchange(
    socket_path: &Path,
    request: &RequestEnvelope,
) -> io::Result<ResponseEnvelope> {
    let mut stream = UnixStream::connect(socket_path)?;
    write_request(&mut stream, request)?;
    let response = read_response(&mut BufReader::new(stream))?;
    if response.version() != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint uses an unsupported control protocol",
        ));
    }
    Ok(response)
}

/// Classifies one command argument without contacting the local endpoint.
#[expect(
    clippy::single_call_fn,
    reason = "Input classification is separate from local control transport"
)]
#[inline]
fn request(
    arguments: &[String],
    current_directory: &Path,
) -> io::Result<RequestEnvelope> {
    let mut values = arguments.iter();
    let Some(text) = values.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: omarchy-quickshare <text|url|file>",
        ));
    };
    if values.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: omarchy-quickshare <text|url|file>",
        ));
    }
    let path = current_directory.join(text);
    if path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder sharing is not supported yet",
        ));
    }
    if path.is_file() {
        return Ok(RequestEnvelope::submit_file(&path));
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return Ok(RequestEnvelope::submit_url(text));
    }
    Ok(RequestEnvelope::submit_text(text))
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
    if let [flag, value] = arguments
        && flag == "--cancel"
    {
        let share_id = value.parse::<u64>().map_err(|_error| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid share ID")
        })?;
        let response =
            exchange(socket_path, &RequestEnvelope::cancel(share_id))?;
        return match response.response() {
            Response::Cancelled => writeln!(output, "Share cancelled."),
            Response::NotFound
            | Response::Queued
            | Response::Ready
            | Response::Snapshot { .. }
            | _ => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "active share was not found",
            )),
        };
    }
    let request = request(arguments, current_directory)?;
    let response = exchange(socket_path, &request)?;
    match response.response() {
        Response::Queued => writeln!(output, "Share queued."),
        Response::Cancelled
        | Response::NotFound
        | Response::Ready
        | Response::Snapshot { .. }
        | _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint returned an unsupported response",
        )),
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
