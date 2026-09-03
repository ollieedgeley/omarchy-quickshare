use std::io::{self, BufRead, Write};

use crate::request::Envelope as RequestEnvelope;
use crate::response::Envelope as ResponseEnvelope;

/// Reads one newline-delimited control request.
///
/// # Errors
///
/// Returns an error when reading or deserialization fails.
#[inline]
pub fn read_request<Reader>(reader: &mut Reader) -> io::Result<RequestEnvelope>
where
    Reader: BufRead,
{
    let mut record = String::new();
    let bytes_read = reader.read_line(&mut record)?;
    if bytes_read == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    serde_json::from_str(&record).map_err(invalid_data)
}

/// Writes one newline-delimited control request.
///
/// # Errors
///
/// Returns an error when serialization or writing fails.
#[inline]
pub fn write_request<Writer>(
    writer: &mut Writer,
    request: &RequestEnvelope,
) -> io::Result<()>
where
    Writer: Write,
{
    serde_json::to_writer(&mut *writer, request).map_err(invalid_data)?;
    writer.write_all(b"\n")
}

/// Reads one newline-delimited control response.
///
/// # Errors
///
/// Returns an error when reading or deserialization fails.
#[inline]
pub fn read_response<Reader>(
    reader: &mut Reader,
) -> io::Result<ResponseEnvelope>
where
    Reader: BufRead,
{
    let mut record = String::new();
    let bytes_read = reader.read_line(&mut record)?;
    if bytes_read == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    serde_json::from_str(&record).map_err(invalid_data)
}

/// Writes one newline-delimited control response.
///
/// # Errors
///
/// Returns an error when serialization or writing fails.
#[inline]
pub fn write_response<Writer>(
    writer: &mut Writer,
    response: &ResponseEnvelope,
) -> io::Result<()>
where
    Writer: Write,
{
    serde_json::to_writer(&mut *writer, response).map_err(invalid_data)?;
    writer.write_all(b"\n")
}

/// Converts malformed JSON into a local protocol I/O error.
fn invalid_data(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
