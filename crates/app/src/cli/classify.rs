//! Classifies `send` content into control-protocol requests.

use std::io;
use std::path::{Path, PathBuf};

use quickshare_control::request::Envelope as RequestEnvelope;

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

/// Classifies one `send` argument without contacting the local endpoint.
pub(super) fn request(
    content: &str,
    current_directory: &Path,
) -> io::Result<RequestEnvelope> {
    classified_request(content, current_directory, None)
}

/// Classifies one `send` argument and targets one observed peer.
pub(super) fn request_for_peer(
    content: &str,
    current_directory: &Path,
    peer_id: &str,
) -> io::Result<RequestEnvelope> {
    classified_request(content, current_directory, Some(peer_id))
}

fn classified_request(
    content: &str,
    current_directory: &Path,
    peer_id: Option<&str>,
) -> io::Result<RequestEnvelope> {
    if let Some(path) = file_uri_path(content)? {
        if path.is_dir() || path.is_file() {
            return Ok(file_request(&path, peer_id));
        }
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file URI path does not exist",
        ));
    }
    let path = current_directory.join(content);
    if path.is_dir() || path.is_file() {
        return Ok(file_request(&path, peer_id));
    }
    if content.starts_with("http://") || content.starts_with("https://") {
        return Ok(url_request(content, peer_id));
    }
    Ok(text_request(content, peer_id))
}

fn file_request(path: &Path, peer_id: Option<&str>) -> RequestEnvelope {
    peer_id.map_or_else(
        || RequestEnvelope::submit_file(path),
        |peer_id| RequestEnvelope::submit_file_to_peer(path, peer_id),
    )
}

fn text_request(text: &str, peer_id: Option<&str>) -> RequestEnvelope {
    peer_id.map_or_else(
        || RequestEnvelope::submit_text(text),
        |peer_id| RequestEnvelope::submit_text_to_peer(text, peer_id),
    )
}

fn url_request(url: &str, peer_id: Option<&str>) -> RequestEnvelope {
    peer_id.map_or_else(
        || RequestEnvelope::submit_url(url),
        |peer_id| RequestEnvelope::submit_url_to_peer(url, peer_id),
    )
}
