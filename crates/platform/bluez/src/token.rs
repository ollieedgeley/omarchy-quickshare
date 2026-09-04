//! Pinned Quick Share weave connect-token encoding.
//!
//! Layer-A `CONNECTION_REQUEST` from the Nearby weave socket:
//! `80 | min_ver(2 BE) | max_ver(2 BE) | max_pkt(2 BE)` followed by the
//! initiator address so fake inboxes can name the peer.

use alloc::collections::VecDeque;

use crate::radio::{Address, Error};

/// Weave `CONNECTION_REQUEST` command with the control bit set.
pub(crate) const WEAVE_CONN_REQUEST: u8 = 0x80;
const VERSION: u16 = 1;
const MAX_PACKET: u16 = 0x01_FD;
const HEADER_LEN: usize = 7;
const TOKEN_LEN: usize = HEADER_LEN + 6;

/// Encodes a weave `CONNECTION_REQUEST` carrying `address`.
#[must_use]
pub(crate) fn encode(address: Address) -> Vec<u8> {
    let mut token = Vec::with_capacity(TOKEN_LEN);
    token.push(WEAVE_CONN_REQUEST);
    token.extend_from_slice(&VERSION.to_be_bytes());
    token.extend_from_slice(&VERSION.to_be_bytes());
    token.extend_from_slice(&MAX_PACKET.to_be_bytes());
    token.extend_from_slice(&address.bytes());
    token
}

/// Parses a weave connect token and returns the initiator address.
///
/// # Errors
///
/// Returns [`Error`] when the header or length does not match the pinned
/// weave `CONNECTION_REQUEST` layout.
pub(crate) fn parse(token: &[u8]) -> Result<Address, Error> {
    if token.len() != TOKEN_LEN || token[0] != WEAVE_CONN_REQUEST {
        return Err(Error::protocol("invalid weave connect token"));
    }
    let min_ver = u16::from_be_bytes([token[1], token[2]]);
    let max_ver = u16::from_be_bytes([token[3], token[4]]);
    if min_ver != VERSION || max_ver < VERSION {
        return Err(Error::protocol("invalid weave connect token"));
    }
    let mut bytes = [0_u8; 6];
    bytes.copy_from_slice(&token[HEADER_LEN..]);
    Ok(Address::from_bytes(bytes))
}

/// Pops the next non-token payload from `inbox`.
pub(crate) fn pop_payload(inbox: &mut VecDeque<Vec<u8>>) -> Option<Vec<u8>> {
    while let Some(bytes) = inbox.pop_front() {
        if parse(&bytes).is_err() {
            return Some(bytes);
        }
    }
    None
}
