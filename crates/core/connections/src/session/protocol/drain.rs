use super::{
    connection_received, frame_dispatch_ignored,
    frames::{disconnect, keepalive},
    keepalive_received, keepalive_sent,
};
use crate::{Connection, Error};
use core::time::Duration;
use quickshare_wire::connections::{
    KeepAliveFrame, V1Frame, payload_transfer_frame, v1_frame,
};
use std::{io, time::Instant};

impl Connection {
    /// Drains authenticated post-transfer control traffic until peer closure.
    ///
    /// This consumes the connection because a deadline or cancellation may
    /// interrupt a partially received frame, after which framing cannot resume.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Cancelled`] when `is_cancelled` requests cancellation,
    /// an I/O timeout when `grace` expires, or a protocol error for malformed,
    /// unauthenticated, or unexpected payload traffic.
    pub fn drain_post_transfer_control<IsCancelled>(
        mut self,
        grace: Duration,
        mut is_cancelled: IsCancelled,
    ) -> Result<(), Error>
    where
        IsCancelled: FnMut() -> bool,
    {
        let deadline = Instant::now().checked_add(grace).ok_or_else(|| {
            Error::Io(io::Error::other("control drain deadline overflow"))
        })?;
        let result = self.drain_control_until(deadline, &mut is_cancelled);
        if result.is_err() {
            self.close_during(deadline);
        }
        result
    }

    fn drain_control_until(
        &mut self,
        deadline: Instant,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        loop {
            let frame = match self.recv_during(deadline, is_cancelled) {
                Err(Error::Io(error)) if peer_closed(error.kind()) => {
                    return Ok(());
                }
                other => other?,
            };
            let v1 = frame.v1.ok_or(Error::UnexpectedFrame)?;
            if self.drain_frame(v1, deadline, is_cancelled)? {
                return Ok(());
            }
        }
    }

    fn drain_frame(
        &mut self,
        v1: V1Frame,
        deadline: Instant,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bool, Error> {
        match v1.r#type {
            Some(value)
                if value == v1_frame::FrameType::Disconnection as i32 =>
            {
                connection_received();
                Ok(true)
            }
            Some(value) if value == v1_frame::FrameType::KeepAlive as i32 => {
                self.drain_keepalive(v1.keep_alive, deadline, is_cancelled)
            }
            Some(value)
                if value == v1_frame::FrameType::PayloadTransfer as i32 =>
            {
                let transfer =
                    v1.payload_transfer.ok_or(Error::UnexpectedFrame)?;
                if transfer.packet_type
                    == Some(payload_transfer_frame::PacketType::Data as i32)
                {
                    return Err(Error::InvalidPayload);
                }
                drop(self.payload(transfer)?);
                Ok(false)
            }
            Some(value)
                if matches!(
                    v1_frame::FrameType::try_from(value),
                    Ok(v1_frame::FrameType::PairedKeyEncryption
                        | v1_frame::FrameType::AuthenticationMessage
                        | v1_frame::FrameType::AuthenticationResult
                        | v1_frame::FrameType::AutoResume
                        | v1_frame::FrameType::AutoReconnect
                        | v1_frame::FrameType::BandwidthUpgradeRetry)
                ) =>
            {
                frame_dispatch_ignored(value);
                Ok(false)
            }
            _ => Err(Error::UnexpectedFrame),
        }
    }

    fn drain_keepalive(
        &mut self,
        frame: Option<KeepAliveFrame>,
        deadline: Instant,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<bool, Error> {
        let frame_data = frame.ok_or(Error::UnexpectedFrame)?;
        let ack = frame_data.ack.unwrap_or(false);
        let sequence = frame_data.seq_num.unwrap_or(0);
        keepalive_received();
        if ack {
            return Ok(false);
        }
        match self.send_during(
            &keepalive(true, sequence),
            deadline,
            is_cancelled,
        ) {
            Ok(()) => {
                keepalive_sent(true);
                Ok(false)
            }
            Err(error @ Error::Io(_)) => self
                .confirm_peer_close(error, deadline, is_cancelled)
                .map(|()| true),
            Err(
                error @ (Error::Wire(_)
                | Error::FrameTooLarge
                | Error::UnexpectedFrame
                | Error::Rejected
                | Error::Handshake
                | Error::Crypto
                | Error::InvalidPayload
                | Error::Cancelled),
            ) => Err(error),
        }
    }

    fn confirm_peer_close(
        &mut self,
        write_error: Error,
        deadline: Instant,
        is_cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<(), Error> {
        let confirmation_deadline = short_deadline(deadline);
        match self.recv_during(confirmation_deadline, is_cancelled) {
            Err(Error::Io(error)) if peer_closed(error.kind()) => Ok(()),
            Err(Error::Io(error))
                if error.kind() == io::ErrorKind::InvalidData =>
            {
                Err(Error::Io(error))
            }
            Err(
                error @ (Error::Crypto
                | Error::Wire(_)
                | Error::FrameTooLarge
                | Error::Cancelled),
            ) => Err(error),
            Err(_) => Err(write_error),
            Ok(frame) => {
                let v1 = frame.v1.ok_or(Error::UnexpectedFrame)?;
                if v1.r#type == Some(v1_frame::FrameType::Disconnection as i32)
                {
                    connection_received();
                    Ok(())
                } else {
                    Err(write_error)
                }
            }
        }
    }

    fn close_during(&mut self, deadline: Instant) {
        let cleanup_deadline = short_deadline(deadline);
        let mut never_cancelled = || false;
        drop(self.send_during(
            &disconnect(),
            cleanup_deadline,
            &mut never_cancelled,
        ));
        drop(self.stream.shutdown_write());
    }
}

const fn peer_closed(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset
    )
}

fn short_deadline(deadline: Instant) -> Instant {
    Instant::now()
        .checked_add(Duration::from_millis(100))
        .map_or(deadline, |value| value.min(deadline))
}
