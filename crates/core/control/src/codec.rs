use std::io::{self, BufRead, Write};

use serde::{Serialize, de::DeserializeOwned};

use crate::MAX_MESSAGE_BYTES;
use crate::request::Envelope as RequestEnvelope;
use crate::response::Envelope as ResponseEnvelope;

/// Serialization buffer that rejects JSON exceeding the frame limit.
struct BoundedRecord(Vec<u8>);

#[expect(
    clippy::missing_trait_methods,
    reason = "Write defaults must use bounded write; some defaults are unstable"
)]
impl Write for BoundedRecord {
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() > MAX_MESSAGE_BYTES.saturating_sub(self.0.len()) {
            return Err(message_too_large());
        }
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
}

/// Reads one bounded, LF-terminated control request, preserving later frames.
///
/// # Errors
///
/// Returns an error for oversized, truncated, malformed, or unreadable frames.
#[inline]
pub fn read_request<Reader>(reader: &mut Reader) -> io::Result<RequestEnvelope>
where
    Reader: BufRead,
{
    let request: RequestEnvelope = read_frame(reader)?;
    request.validate()?;
    Ok(request)
}

/// Writes one bounded, LF-terminated control request.
///
/// # Errors
///
/// Returns an error for oversized messages, serialization, or failed writes.
#[inline]
pub fn write_request<Writer>(
    writer: &mut Writer,
    request: &RequestEnvelope,
) -> io::Result<()>
where
    Writer: Write,
{
    request.validate()?;
    write_frame(writer, request)
}

/// Reads one bounded, LF-terminated control response, preserving later frames.
///
/// # Errors
///
/// Returns an error for oversized, truncated, malformed, or unreadable frames.
#[inline]
pub fn read_response<Reader>(
    reader: &mut Reader,
) -> io::Result<ResponseEnvelope>
where
    Reader: BufRead,
{
    read_frame(reader)
}

/// Writes one bounded, LF-terminated control response.
///
/// # Errors
///
/// Returns an error for oversized messages, serialization, or failed writes.
#[inline]
pub fn write_response<Writer>(
    writer: &mut Writer,
    response: &ResponseEnvelope,
) -> io::Result<()>
where
    Writer: Write,
{
    write_frame(writer, response)
}

/// Reads JSON up to one LF without consuming bytes from the following frame.
fn read_frame<Reader, Message>(reader: &mut Reader) -> io::Result<Message>
where
    Reader: BufRead,
    Message: DeserializeOwned,
{
    let mut record = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated control frame",
            ));
        }
        let chunk = available
            .split(|byte| *byte == b'\n')
            .next()
            .unwrap_or_default();
        let length = chunk.len();
        let has_newline = length < available.len();
        if length > MAX_MESSAGE_BYTES.saturating_sub(record.len()) {
            return Err(message_too_large());
        }
        record.extend_from_slice(chunk);
        reader.consume(length);
        if has_newline {
            reader.consume(1);
            return serde_json::from_slice(&record).map_err(invalid_data);
        }
    }
}

/// Serializes into a bounded buffer before writing any bytes to the transport.
fn write_frame<Writer, Message>(
    writer: &mut Writer,
    message: &Message,
) -> io::Result<()>
where
    Writer: Write,
    Message: Serialize,
{
    let mut record = BoundedRecord(Vec::new());
    serde_json::to_writer(&mut record, message).map_err(|error| {
        error
            .io_error_kind()
            .map_or_else(|| invalid_data(error), io::Error::from)
    })?;
    writer.write_all(&record.0)?;
    writer.write_all(b"\n")
}

/// Constructs the error shared by incoming and outgoing oversized frames.
fn message_too_large() -> io::Error {
    io::Error::new(
        io::ErrorKind::FileTooLarge,
        "control frame exceeds MAX_MESSAGE_BYTES",
    )
}

/// Converts malformed JSON into the transport's invalid-data error kind.
fn invalid_data(error: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
