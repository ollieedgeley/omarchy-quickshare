//! Classifies CLI arguments into control-protocol requests.

use std::env;
use std::io;
use std::path::{Path, PathBuf};

use quickshare_control::request::Envelope as RequestEnvelope;

/// Parses one state-changing CLI command.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed CLI arguments retain their string ownership"
)]
pub(super) fn action_request(
    arguments: &[String],
) -> io::Result<Option<RequestEnvelope>> {
    let request = match arguments {
        [flag, value] if flag == "--accept" => {
            RequestEnvelope::accept(parse_number(value, "share ID")?)
        }
        [flag, value] if flag == "--cancel" => {
            RequestEnvelope::cancel(parse_number(value, "share ID")?)
        }
        [flag] if flag == "--close-visibility" => {
            RequestEnvelope::close_visibility()
        }
        [flag] if flag == "--discover" => RequestEnvelope::discover(),
        [flag, value] if flag == "--dismiss" => {
            RequestEnvelope::dismiss(parse_number(value, "share ID")?)
        }
        [flag] if flag == "--open-visibility" => {
            RequestEnvelope::open_visibility()
        }
        [flag, peer_id] if flag == "--pin" => {
            RequestEnvelope::pin_peer(peer_id)
        }
        [flag, value] if flag == "--reject" => {
            RequestEnvelope::reject(parse_number(value, "share ID")?)
        }
        [flag, share_id, peer_id] if flag == "--send-to" => {
            RequestEnvelope::select_peer(
                parse_number(share_id, "share ID")?,
                peer_id,
            )
        }
        [flag] if flag == "--stop-discovery" => {
            RequestEnvelope::stop_discovery()
        }
        [flag] if flag == "--unpin" => RequestEnvelope::unpin_peer(),
        _ => return simulation_action_request(arguments),
    };
    Ok(Some(request))
}

/// Parses one simulator-only peer or transport event.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "Borrowed simulator arguments retain their string ownership"
)]
fn simulation_action_request(
    arguments: &[String],
) -> io::Result<Option<RequestEnvelope>> {
    if !arguments
        .first()
        .is_some_and(|flag| flag.starts_with("--simulate-"))
    {
        return Ok(None);
    }
    if env::var_os("OMARCHY_QUICKSHARE_ALLOW_SIMULATION").is_none() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "simulation commands are unavailable; start a simulated daemon \
             with --daemon --simulate and set \
             OMARCHY_QUICKSHARE_ALLOW_SIMULATION=1",
        ));
    }
    let request = match arguments {
        [flag] if flag == "--simulate-discovery-timeout" => {
            RequestEnvelope::simulate_discovery_timeout()
        }
        [flag, value] if flag == "--simulate-fail" => {
            RequestEnvelope::simulate_fail(parse_number(value, "share ID")?)
        }
        [flag, name, size] if flag == "--simulate-incoming-file" => {
            RequestEnvelope::simulate_incoming_file(
                name,
                parse_number(size, "byte count")?,
            )
        }
        [flag, text] if flag == "--simulate-incoming-text" => {
            RequestEnvelope::simulate_incoming_text(text)
        }
        [flag, url] if flag == "--simulate-incoming-url" => {
            RequestEnvelope::simulate_incoming_url(url)
        }
        [flag, value] if flag == "--simulate-peer-accept" => {
            RequestEnvelope::simulate_peer_accept(parse_number(
                value, "share ID",
            )?)
        }
        [flag, peer_id] if flag == "--simulate-peer-lost" => {
            RequestEnvelope::simulate_peer_lost(peer_id)
        }
        [flag, value] if flag == "--simulate-peer-reject" => {
            RequestEnvelope::simulate_peer_reject(parse_number(
                value, "share ID",
            )?)
        }
        [flag, peer_id, name] if flag == "--simulate-peer-seen" => {
            RequestEnvelope::simulate_peer_seen(peer_id, name)
        }
        [flag, share_id, transferred] if flag == "--simulate-progress" => {
            RequestEnvelope::simulate_progress(
                parse_number(share_id, "share ID")?,
                parse_number(transferred, "byte count")?,
            )
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unrecognized simulation command",
            ));
        }
    };
    Ok(Some(request))
}
/// Parses a non-negative control protocol integer.
fn parse_number(value: &str, field: &str) -> io::Result<u64> {
    value.parse::<u64>().map_err(|_error| {
        io::Error::new(io::ErrorKind::InvalidInput, format!("invalid {field}"))
    })
}

/// Treats one local `file://` URI as an absolute filesystem path.
fn file_uri_path(text: &str) -> io::Result<Option<PathBuf>> {
    let lines = file_uri_lines(text);
    if !lines.iter().any(|line| line.starts_with("file:")) {
        return Ok(None);
    }
    match lines.as_slice() {
        [uri] => decode_local_file_uri(uri).map(Some),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI list has multiple entries",
        )),
    }
}

/// Drops comments and blank lines from a `text/uri-list` argument.
fn file_uri_lines(text: &str) -> Vec<&str> {
    text.split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// Percent-decodes one local `file:` URI and rejects remote hosts.
fn decode_local_file_uri(uri: &str) -> io::Result<PathBuf> {
    let rest = uri.strip_prefix("file:").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "file URI is malformed")
    })?;
    let path = if let Some(rest) = rest.strip_prefix("//") {
        if rest.starts_with('/') {
            rest
        } else {
            let Some(slash) = rest.find('/') else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "file URI is malformed",
                ));
            };
            let host = percent_decode_utf8(&rest[..slash])?;
            if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "file URI host is not local",
                ));
            }
            &rest[slash..]
        }
    } else if rest.starts_with('/') {
        rest
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI is malformed",
        ));
    };
    if path.contains(['?', '#']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI is malformed",
        ));
    }
    let path = percent_decode_utf8(path)?;
    if !path.starts_with('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI is malformed",
        ));
    }
    Ok(PathBuf::from(path))
}

/// Percent-decodes URI bytes and rejects NULs or invalid UTF-8.
fn percent_decode_utf8(input: &str) -> io::Result<String> {
    let decoded = percent_decode(input)?;
    if decoded.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI contains a NUL",
        ));
    }
    String::from_utf8(decoded).map_err(|_error| {
        io::Error::new(io::ErrorKind::InvalidInput, "file URI is malformed")
    })
}

/// Decodes `%HH` escapes without treating `+` as space.
fn percent_decode(input: &str) -> io::Result<Vec<u8>> {
    let mut bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    while let Some((first, rest)) = bytes.split_first() {
        if *first == b'%' {
            let [high, low, remainder @ ..] = rest else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "file URI is malformed",
                ));
            };
            decoded.push((hex_digit(*high)? << 4) | hex_digit(*low)?);
            bytes = remainder;
            continue;
        }
        decoded.push(*first);
        bytes = rest;
    }
    Ok(decoded)
}

/// Parses one hexadecimal digit from a percent-escape.
fn hex_digit(byte: u8) -> io::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI is malformed",
        )),
    }
}

/// Classifies one command argument without contacting the local endpoint.
pub(super) fn request(
    arguments: &[String],
    current_directory: &Path,
) -> io::Result<RequestEnvelope> {
    let mut values = arguments.iter();
    let Some(text) = values.next() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: omarchy-quickshare <text|url|file|folder>",
        ));
    };
    if values.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: omarchy-quickshare <text|url|file|folder>",
        ));
    }
    if let Some(path) = file_uri_path(text)? {
        if path.is_dir() || path.is_file() {
            return Ok(RequestEnvelope::submit_file(&path));
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI path does not exist",
        ));
    }
    let path = current_directory.join(text);
    if path.is_dir() || path.is_file() {
        return Ok(RequestEnvelope::submit_file(&path));
    }
    if text.starts_with("http://") || text.starts_with("https://") {
        return Ok(RequestEnvelope::submit_url(text));
    }
    Ok(RequestEnvelope::submit_text(text))
}
