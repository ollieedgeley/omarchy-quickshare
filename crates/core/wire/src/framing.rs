use core::{error, fmt};

/// The fixed width of the protocol length prefix.
const PREFIX_LENGTH: usize = 4;

/// A framing failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
#[expect(
    clippy::error_impl_error,
    reason = "The public error follows the framing module API"
)]
pub enum Error {
    /// The bytes do not contain one complete frame.
    Incomplete,
    /// The payload is larger than the caller's configured limit.
    LimitExceeded,
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Incomplete => f.write_str("frame is incomplete"),
            Self::LimitExceeded => {
                f.write_str("frame exceeds its configured limit")
            }
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "The public error has no source or custom Error metadata"
)]
impl error::Error for Error {}

/// Prefixes `payload` with its four-byte big-endian length.
///
/// # Errors
///
/// Returns an error when `payload` is larger than `limit` or cannot fit in a
/// four-byte length prefix.
#[inline]
#[expect(
    clippy::big_endian_bytes,
    reason = "Quick Share framing uses a four-byte big-endian length prefix"
)]
pub fn encode(payload: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    let length =
        u32::try_from(payload.len()).map_err(|_error| Error::LimitExceeded)?;
    if payload.len() > limit {
        return Err(Error::LimitExceeded);
    }
    let capacity = PREFIX_LENGTH
        .checked_add(payload.len())
        .ok_or(Error::LimitExceeded)?;
    let mut frame = Vec::with_capacity(capacity);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

/// Decodes one complete four-byte big-endian length-prefixed frame.
///
/// # Errors
///
/// Returns an error for an incomplete frame or a declared length above `limit`.
#[inline]
#[expect(
    clippy::big_endian_bytes,
    reason = "Quick Share framing uses a four-byte big-endian length prefix"
)]
pub fn decode(frame: &[u8], limit: usize) -> Result<&[u8], Error> {
    let Some(prefix) = frame.get(..PREFIX_LENGTH) else {
        return Err(Error::Incomplete);
    };
    let prefix_bytes: &[u8; PREFIX_LENGTH] =
        prefix.try_into().map_err(|_error| Error::Incomplete)?;
    let declared_length = u32::from_be_bytes(*prefix_bytes);
    let payload_length = usize::try_from(declared_length)
        .map_err(|_error| Error::LimitExceeded)?;
    if payload_length > limit {
        return Err(Error::LimitExceeded);
    }
    let payload = frame.get(PREFIX_LENGTH..).ok_or(Error::Incomplete)?;
    if payload.len() != payload_length {
        return Err(Error::Incomplete);
    }
    Ok(payload)
}
