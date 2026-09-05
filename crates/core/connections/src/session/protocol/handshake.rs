use super::super::{
    Connection, ConnectionIo, ConnectionOptions, Error, Medium, UpgradeState,
};
use super::{
    frames::{request, request_data, response, response_data},
    io::{read, receive_plain, send_plain, write},
};
use core::time::Duration;
use quickshare_crypto::{CompletedHandshake, Handshake};
use std::{
    collections::{HashMap, VecDeque},
    io,
    net::TcpStream,
    time::Instant,
};

impl Connection {
    /// Establishes encryption for the initiating side of a TCP connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when framing, UKEY2, or the peer's response fails.
    pub fn connect(
        stream: TcpStream,
        handshake: Handshake,
        options: ConnectionOptions,
    ) -> Result<Self, Error> {
        Self::connect_io(stream, handshake, options)
    }

    /// Establishes encryption for the initiating side of a byte stream.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when framing, UKEY2, or the peer's response fails.
    pub fn connect_io<Stream>(
        mut stream: Stream,
        mut handshake: Handshake,
        options: ConnectionOptions,
    ) -> Result<Self, Error>
    where
        Stream: ConnectionIo + 'static,
    {
        let medium = options.medium;
        let endpoint_id = options.id.clone();
        send_plain(&mut stream, &request(options))?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "send",
            outcome = "locally_written",
            frame_type = "connection_request",
            "setup frame sent"
        );
        send_ukey2(&mut stream, &mut handshake, "client_init")?;
        receive_ukey2(&mut stream, &mut handshake, "server_init")?;
        send_ukey2(&mut stream, &mut handshake, "client_finish")?;
        send_plain(&mut stream, &response())?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "send",
            outcome = "locally_written",
            frame_type = "connection_response",
            "setup frame sent"
        );
        response_data(receive_plain(&mut stream)?)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "receive",
            outcome = "completed",
            frame_type = "connection_response",
            "setup frame received"
        );
        Ok(Self::new(
            Box::new(stream),
            handshake.complete().map_err(|_| Error::Handshake)?,
            medium,
            endpoint_id,
        ))
    }

    /// Establishes encryption for the accepting side of a TCP connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when framing, UKEY2, or the peer's request fails.
    pub fn accept(
        stream: TcpStream,
        handshake: Handshake,
        options: ConnectionOptions,
    ) -> Result<Self, Error> {
        Self::accept_io(stream, handshake, options)
    }

    /// Establishes encryption for the accepting side of a byte stream.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when framing, UKEY2, or the peer's request fails.
    pub fn accept_io<Stream>(
        mut stream: Stream,
        mut handshake: Handshake,
        options: ConnectionOptions,
    ) -> Result<Self, Error>
    where
        Stream: ConnectionIo + 'static,
    {
        let medium = options.medium;
        let endpoint_id = options.id;
        request_data(receive_plain(&mut stream)?)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "receive",
            outcome = "completed",
            frame_type = "connection_request",
            "setup frame received"
        );
        receive_ukey2(&mut stream, &mut handshake, "client_init")?;
        send_ukey2(&mut stream, &mut handshake, "server_init")?;
        receive_ukey2(&mut stream, &mut handshake, "client_finish")?;
        send_plain(&mut stream, &response())?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "send",
            outcome = "locally_written",
            frame_type = "connection_response",
            "setup frame sent"
        );
        response_data(receive_plain(&mut stream)?)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "setup",
            operation = "receive",
            outcome = "completed",
            frame_type = "connection_response",
            "setup frame received"
        );
        Ok(Self::new(
            Box::new(stream),
            handshake.complete().map_err(|_| Error::Handshake)?,
            medium,
            endpoint_id,
        ))
    }

    /// Returns the shared four-digit UKEY2 peer-verification code.
    #[must_use]
    pub fn verification_code(&self) -> &str {
        &self.verification_code
    }

    /// Bounds each subsequent blocking read to `timeout`.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the active stream cannot apply the timeout.
    pub fn set_read_timeout(&mut self, timeout: Duration) -> Result<(), Error> {
        let deadline =
            Instant::now().checked_add(timeout).ok_or_else(|| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "connection",
                    operation = "set_read_deadline",
                    outcome = "rejected",
                    reason = "deadline_overflow",
                    "connection deadline rejected"
                );
                io::Error::other("read deadline overflow")
            })?;
        self.stream.set_read_timeout(timeout).inspect_err(|error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "connection",
                operation = "set_read_deadline",
                outcome = "rejected",
                reason = "io",
                io_error_kind = super::io::io_error_kind(error.kind()),
                "connection deadline rejected"
            );
        })?;
        self.read_deadline = Some(deadline);
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "connection",
            operation = "set_read_deadline",
            outcome = "completed",
            "connection read deadline set"
        );
        Ok(())
    }

    fn new(
        stream: Box<dyn ConnectionIo>,
        completed: CompletedHandshake,
        medium: Medium,
        endpoint_id: String,
    ) -> Self {
        let verification_code =
            verification_code(completed.authentication_token());
        let channel = completed.into_channel();
        Self {
            stream,
            read_deadline: None,
            channel,
            incoming_bytes: HashMap::default(),
            payloads: HashMap::default(),
            incoming_file: None,
            outgoing_file: None,
            pending_events: VecDeque::default(),
            medium,
            upgrade: UpgradeState::Idle,
            upgrade_host: false,
            endpoint_id,
            verification_code,
        }
    }
}

fn send_ukey2<Stream>(
    stream: &mut Stream,
    handshake: &mut Handshake,
    event_type: &'static str,
) -> Result<(), Error>
where
    Stream: io::Write + ?Sized,
{
    let message = handshake.next_message().map_err(|_| Error::Handshake)?;
    write(stream, &message)?;
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "ukey2",
        operation = "send",
        outcome = "locally_written",
        event_type,
        "UKEY2 step sent"
    );
    Ok(())
}

fn receive_ukey2<Stream>(
    stream: &mut Stream,
    handshake: &mut Handshake,
    event_type: &'static str,
) -> Result<(), Error>
where
    Stream: io::Read + ?Sized,
{
    let message = read(stream)?;
    handshake.receive(&message).map_err(|_| Error::Handshake)?;
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "ukey2",
        operation = "receive",
        outcome = "completed",
        event_type,
        "UKEY2 step received"
    );
    Ok(())
}

/// Matches Nearby Connections' decimal rendering of its raw UKEY2 token.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division_remainder_used,
    clippy::modulo_arithmetic,
    reason = "Pinned Google hash requires bounded signed C++ remainder"
)]
fn verification_code(authentication_token: &[u8; 32]) -> String {
    const HASH_MODULUS: i32 = 9_973;
    const HASH_MULTIPLIER: i32 = 31;

    let mut hash = 0_i32;
    let mut multiplier = 1_i32;
    for byte in authentication_token {
        let signed = i8::from_be_bytes([*byte]);
        hash = (hash + i32::from(signed) * multiplier) % HASH_MODULUS;
        multiplier = multiplier * HASH_MULTIPLIER % HASH_MODULUS;
    }
    format!("{:04}", hash.abs())
}
