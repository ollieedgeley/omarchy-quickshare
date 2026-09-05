//! Nearby weave layer-A/B framing used on GATT write/notify sockets.
//!
//! Layer A is one header byte per GATT PDU: control bit, 3-bit counter,
//! first/last flags, and a 2-bit command. Layer B prefixes reassembled
//! payloads with a 3-byte service-id hash (`fc9f5e` data, `000000` control).

use std::time::{Duration, Instant};

use crate::radio::{Error, ErrorKind};

/// Control bit in a layer-A header.
pub(crate) const CONTROL: u8 = 0x80;
/// First-fragment bit in a layer-A data header.
const FIRST: u8 = 0x08;
/// Last-fragment bit in a layer-A data header.
const LAST: u8 = 0x04;
const COUNTER_SHIFT: u32 = 4;
const COUNTER_MASK: u8 = 0x07;
const CMD_MASK: u8 = 0x03;
const CMD_REQUEST: u8 = 0;
const CMD_CONFIRM: u8 = 1;
const CMD_ERROR: u8 = 2;
const VERSION: u16 = 1;
/// Documented weave max GATT PDU size, including the layer-A header.
pub(crate) const MAX_PACKET: u16 = 0x01_FD;
const REQUEST_LEN: usize = 7;
const CONFIRM_LEN: usize = 5;
const HASH_LEN: usize = 3;
/// SHA-256("NearbySharing")[0:3].
const DATA_HASH: [u8; HASH_LEN] = [0xFC, 0x9F, 0x5E];
const CONTROL_HASH: [u8; HASH_LEN] = [0, 0, 0];

/// Layer-A connection request (`80 0001 0001 01fd`).
#[must_use]
pub(crate) fn encode_request() -> Vec<u8> {
    let mut packet = Vec::with_capacity(REQUEST_LEN);
    packet.push(CONTROL | CMD_REQUEST);
    packet.extend_from_slice(&VERSION.to_be_bytes());
    packet.extend_from_slice(&VERSION.to_be_bytes());
    packet.extend_from_slice(&MAX_PACKET.to_be_bytes());
    packet
}

/// Layer-A connection confirm (`81 0001 01fd`).
#[must_use]
pub(crate) fn encode_confirm() -> Vec<u8> {
    let mut packet = Vec::with_capacity(CONFIRM_LEN);
    packet.push(CONTROL | CMD_CONFIRM);
    packet.extend_from_slice(&VERSION.to_be_bytes());
    packet.extend_from_slice(&MAX_PACKET.to_be_bytes());
    packet
}

/// Encodes `payload` as layer-B data fragmented into layer-A GATT PDUs.
#[must_use]
pub(crate) fn encode_data(payload: &[u8], max_chunk: usize) -> Vec<Vec<u8>> {
    let mut body = Vec::with_capacity(HASH_LEN.saturating_add(payload.len()));
    body.extend_from_slice(&DATA_HASH);
    body.extend_from_slice(payload);
    fragment(&body, max_chunk.max(1))
}

/// Parses one GATT PDU into a layer-A packet.
pub(crate) fn decode_pdu(pdu: &[u8]) -> Result<Packet, Error> {
    let header = pdu
        .first()
        .copied()
        .ok_or_else(|| Error::protocol("truncated weave layer-A header"))?;
    let rest = pdu.get(1..).unwrap_or(&[]);
    if header & CONTROL == 0 {
        return Ok(Packet::Data {
            first: header & FIRST != 0,
            last: header & LAST != 0,
            payload: rest.to_vec(),
        });
    }
    decode_control(header, rest)
}

/// Reassembles layer-A data PDUs into one layer-B payload.
#[derive(Debug)]
pub(crate) struct Assembler {
    /// Bytes collected between first and last.
    buffer: Vec<u8>,
    /// Whether a first fragment has been seen.
    started: bool,
}

/// One decoded layer-A packet.
#[derive(Debug)]
pub(crate) enum Packet {
    /// `CONNECTION_REQUEST`.
    Request,
    /// `CONNECTION_CONFIRM`.
    Confirm,
    /// `ERROR`.
    Error,
    /// Data fragment.
    Data {
        /// Start of a layer-B message.
        first: bool,
        /// End of a layer-B message.
        last: bool,
        /// Bytes after the layer-A header.
        payload: Vec<u8>,
    },
}

/// A complete layer-B message.
#[derive(Debug)]
pub(crate) enum Message {
    /// Connections bytes (`fc9f5e` stripped).
    Data(Vec<u8>),
    /// Socket-control protobuf (`000000` stripped).
    Control(Vec<u8>),
}

impl Default for Assembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Assembler {
    /// Creates an empty reassembly buffer.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self {
            buffer: Vec::new(),
            started: false,
        }
    }

    /// Pushes one GATT PDU. Returns a message when `last` arrives.
    pub(crate) fn push(
        &mut self,
        pdu: &[u8],
    ) -> Result<Option<Message>, Error> {
        match decode_pdu(pdu)? {
            Packet::Request | Packet::Confirm | Packet::Error => Ok(None),
            Packet::Data {
                first,
                last,
                payload,
            } => self.push_data(first, last, payload),
        }
    }

    fn push_data(
        &mut self,
        first: bool,
        last: bool,
        payload: Vec<u8>,
    ) -> Result<Option<Message>, Error> {
        if first {
            if self.started {
                return Err(Error::protocol("nested weave first fragment"));
            }
            self.started = true;
            self.buffer.clear();
        } else if !self.started {
            return Err(Error::protocol("weave fragment missing first"));
        }
        self.buffer.extend_from_slice(&payload);
        if !last {
            return Ok(None);
        }
        self.started = false;
        let body = core::mem::take(&mut self.buffer);
        decode_layer_b(&body).map(Some)
    }
}

/// Sends a connection request once, then one layer-B data payload.
pub(crate) fn send_data<F>(
    payload: &[u8],
    requested: &mut bool,
    mut write_pdu: F,
) -> Result<(), Error>
where
    F: FnMut(&[u8]) -> Result<(), Error>,
{
    if !*requested {
        write_pdu(&encode_request())?;
        *requested = true;
    }
    let max_chunk = usize::from(MAX_PACKET).saturating_sub(1);
    for pdu in encode_data(payload, max_chunk) {
        write_pdu(&pdu)?;
    }
    Ok(())
}

/// Reads GATT PDUs, answers layer-A handshake, and returns one data payload.
pub(crate) fn recv_data<R, W>(
    assembler: &mut Assembler,
    timeout: Duration,
    mut read_pdu: R,
    mut write_pdu: W,
) -> Result<Vec<u8>, Error>
where
    R: FnMut(Instant) -> Result<Vec<u8>, Error>,
    W: FnMut(&[u8]) -> Result<(), Error>,
{
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(Error::timeout)?;
    loop {
        let pdu = match read_pdu(deadline) {
            Err(error)
                if error.kind() == ErrorKind::Closed && assembler.started =>
            {
                return Err(Error::protocol(
                    "truncated weave message at pipe close",
                ));
            }
            result => result?,
        };
        match decode_pdu(&pdu)? {
            Packet::Request => write_pdu(&encode_confirm())?,
            Packet::Confirm | Packet::Error => {}
            Packet::Data { .. } => match assembler.push(&pdu)? {
                Some(Message::Data(bytes)) => return Ok(bytes),
                Some(Message::Control(bytes)) => {
                    if is_disconnect(&bytes) {
                        return Err(Error::closed());
                    }
                }
                None => {}
            },
        }
    }
}

fn decode_control(header: u8, rest: &[u8]) -> Result<Packet, Error> {
    match header & CMD_MASK {
        CMD_REQUEST => {
            if rest.len() != REQUEST_LEN.saturating_sub(1) {
                return Err(Error::protocol(
                    "malformed weave CONNECTION_REQUEST length",
                ));
            }
            Ok(Packet::Request)
        }
        CMD_CONFIRM => {
            if rest.len() != CONFIRM_LEN.saturating_sub(1) {
                return Err(Error::protocol(
                    "malformed weave CONNECTION_CONFIRM length",
                ));
            }
            Ok(Packet::Confirm)
        }
        CMD_ERROR => Ok(Packet::Error),
        _ => Err(Error::protocol("unknown weave layer-A command")),
    }
}

fn decode_layer_b(body: &[u8]) -> Result<Message, Error> {
    if body.len() < HASH_LEN {
        return Err(Error::protocol("malformed weave layer-B length"));
    }
    let hash = [body[0], body[1], body[2]];
    let payload = body[HASH_LEN..].to_vec();
    if hash == DATA_HASH {
        Ok(Message::Data(payload))
    } else if hash == CONTROL_HASH {
        Ok(Message::Control(payload))
    } else {
        Err(Error::protocol("unknown weave layer-B type"))
    }
}

fn fragment(body: &[u8], max_chunk: usize) -> Vec<Vec<u8>> {
    if body.is_empty() {
        return vec![vec![data_header(0, true, true)]];
    }
    let mut packets = Vec::new();
    let mut offset = 0;
    let mut counter = 0_u8;
    while offset < body.len() {
        let end = offset.saturating_add(max_chunk).min(body.len());
        let first = offset == 0;
        let last = end == body.len();
        let mut pdu =
            Vec::with_capacity(end.saturating_sub(offset).saturating_add(1));
        pdu.push(data_header(counter, first, last));
        pdu.extend_from_slice(&body[offset..end]);
        packets.push(pdu);
        offset = end;
        counter = counter.wrapping_add(1) & COUNTER_MASK;
    }
    packets
}

const fn data_header(counter: u8, first: bool, last: bool) -> u8 {
    let mut header = (counter & COUNTER_MASK) << COUNTER_SHIFT;
    if first {
        header |= FIRST;
    }
    if last {
        header |= LAST;
    }
    header
}

fn is_disconnect(control: &[u8]) -> bool {
    control.first() == Some(&0x08) && control.get(1) == Some(&0x02)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::{
        Assembler, Message, decode_pdu, encode_confirm, encode_data,
        encode_request, recv_data,
    };
    use crate::radio::ErrorKind;

    #[test]
    fn request_and_confirm_match_documented_bytes() {
        assert_eq!(
            encode_request(),
            [0x80, 0x00, 0x01, 0x00, 0x01, 0x01, 0xFD]
        );
        assert_eq!(encode_confirm(), [0x81, 0x00, 0x01, 0x01, 0xFD]);
    }

    #[test]
    fn data_round_trip_coalesced_and_fragmented() {
        let payload = b"hello-quick-share";
        let coalesced = encode_data(payload, 64);
        assert_eq!(coalesced.len(), 1);
        let mut assembler = Assembler::new();
        match assembler.push(&coalesced[0]).expect("coalesced") {
            Some(Message::Data(bytes)) => assert_eq!(bytes, payload),
            _ => panic!("expected data"),
        }
        let fragments = encode_data(payload, 4);
        assert!(fragments.len() > 1);
        let mut assembler = Assembler::new();
        let mut decoded = None;
        for fragment in &fragments {
            decoded = assembler.push(fragment).expect("fragment");
        }
        match decoded {
            Some(Message::Data(bytes)) => assert_eq!(bytes, payload),
            _ => panic!("expected reassembled data"),
        }
    }
    #[test]
    fn control_pdus_do_not_extend_the_read_deadline() {
        let timeout = Duration::from_millis(3);
        let mut now = None;
        let mut pdus = VecDeque::from([
            encode_request(),
            encode_confirm(),
            encode_confirm(),
            encode_data(b"too late", 64).remove(0),
        ]);
        let error = recv_data(
            &mut Assembler::new(),
            timeout,
            |read_deadline| {
                let now = now.get_or_insert_with(|| {
                    read_deadline
                        .checked_sub(timeout)
                        .expect("deadline includes timeout")
                });
                if *now >= read_deadline {
                    return Err(crate::radio::Error::timeout());
                }
                *now += Duration::from_millis(1);
                Ok(pdus.pop_front().expect("queued PDU"))
            },
            |_| Ok(()),
        )
        .expect_err("control PDUs must not reset the deadline");

        assert_eq!(error.kind(), ErrorKind::Timeout);
    }

    #[test]
    fn graceful_disconnect_is_closed_not_protocol_failure() {
        let error = recv_data(
            &mut Assembler::new(),
            Duration::from_secs(1),
            |_| Ok(vec![0x0C, 0x00, 0x00, 0x00, 0x08, 0x02]),
            |_| Ok(()),
        )
        .expect_err("disconnect");

        assert_eq!(error.kind(), ErrorKind::Closed);
    }

    #[test]
    fn pipe_close_is_clean_only_between_weave_messages() {
        let boundary = recv_data(
            &mut Assembler::new(),
            Duration::from_secs(1),
            |_| Err(crate::radio::Error::closed()),
            |_| Ok(()),
        )
        .expect_err("boundary close");
        assert_eq!(boundary.kind(), ErrorKind::Closed);

        let mut pdus = VecDeque::from([
            Ok(vec![0x08, 0xFC, 0x9F]),
            Err(crate::radio::Error::closed()),
        ]);
        let truncated = recv_data(
            &mut Assembler::new(),
            Duration::from_secs(1),
            |_| pdus.pop_front().expect("queued pipe result"),
            |_| Ok(()),
        )
        .expect_err("close during fragmented weave message");
        assert_eq!(truncated.kind(), ErrorKind::Protocol);
    }

    #[test]
    fn malformed_lengths_and_types_fail() {
        assert_eq!(
            decode_pdu(&[]).expect_err("empty").kind(),
            ErrorKind::Protocol
        );
        assert_eq!(
            decode_pdu(&[0x80, 0x00]).expect_err("short request").kind(),
            ErrorKind::Protocol
        );
        assert_eq!(
            decode_pdu(&[0x83]).expect_err("bad cmd").kind(),
            ErrorKind::Protocol
        );
        let mut assembler = Assembler::new();
        let error = assembler
            .push(&[0x0C, 0x01, 0x02, 0x03])
            .expect_err("unknown hash");
        assert_eq!(error.kind(), ErrorKind::Protocol);
        let mut assembler = Assembler::new();
        let error = assembler.push(&[0x0C, 0xFC]).expect_err("short layer B");
        assert_eq!(error.kind(), ErrorKind::Protocol);
        let mut assembler = Assembler::new();
        let error = assembler
            .push(&[0x04, 0xFC, 0x9F, 0x5E])
            .expect_err("missing first");
        assert_eq!(error.kind(), ErrorKind::Protocol);
    }
}
