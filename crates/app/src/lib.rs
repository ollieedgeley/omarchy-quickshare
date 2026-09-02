//! Command-line and daemon composition for Omarchy Quick Share.

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        dead_code_pub_in_binary,
        reason = "Library test harness cannot see downstream command use"
    )
)]

use std::env;
use std::io;
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use quickshare_control::PROTOCOL_VERSION;
use quickshare_control::codec::{read_response, write_request};
use quickshare_control::request::Envelope as RequestEnvelope;
use quickshare_control::response::Response;

/// Runs one command against the local endpoint.
///
/// # Errors
///
/// Returns an I/O error when local control cannot complete the command.
#[inline]
pub fn run<Output>(
    arguments: &[String],
    socket_path: &Path,
    output: &mut Output,
) -> io::Result<()>
where
    Output: Write,
{
    let mut values = arguments.iter();
    let Some(text) = values.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: omarchy-quickshare <text>",
        ));
    };
    if values.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: omarchy-quickshare <text>",
        ));
    }
    let mut stream = UnixStream::connect(socket_path)?;
    write_request(&mut stream, &RequestEnvelope::submit_text(text))?;
    let response = read_response(&mut BufReader::new(stream))?;
    if response.version() != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "endpoint uses an unsupported control protocol",
        ));
    }
    match *response.response() {
        Response::Queued => writeln!(output, "Share queued."),
        _ => Err(io::Error::new(
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
    let socket_path = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|root| root.join("omarchy-quickshare/control.sock"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not available",
            )
        })?;
    let stdout = io::stdout();
    run(&arguments, &socket_path, &mut stdout.lock())
}
