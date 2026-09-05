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
        write(
            &mut stream,
            &handshake.next_message().map_err(|_| Error::Handshake)?,
        )?;
        handshake
            .receive(&read(&mut stream)?)
            .map_err(|_| Error::Handshake)?;
        write(
            &mut stream,
            &handshake.next_message().map_err(|_| Error::Handshake)?,
        )?;
        send_plain(&mut stream, &response())?;
        response_data(receive_plain(&mut stream)?)?;
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
        handshake
            .receive(&read(&mut stream)?)
            .map_err(|_| Error::Handshake)?;
        write(
            &mut stream,
            &handshake.next_message().map_err(|_| Error::Handshake)?,
        )?;
        handshake
            .receive(&read(&mut stream)?)
            .map_err(|_| Error::Handshake)?;
        send_plain(&mut stream, &response())?;
        response_data(receive_plain(&mut stream)?)?;
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
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::other("read deadline overflow"))?;
        self.stream.set_read_timeout(timeout)?;
        self.read_deadline = Some(deadline);
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
