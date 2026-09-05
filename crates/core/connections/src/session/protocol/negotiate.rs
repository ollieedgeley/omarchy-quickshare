use super::super::{
    Connection, ConnectionIo, Error, Event, MAX_FRAME_LENGTH, Medium,
    UpgradeCredentials, UpgradeEvent, UpgradeState,
};
use super::frames::{
    client_introduction, client_introduction_ack, last_write_to_prior_channel,
    safe_to_close_prior_channel, upgrade_failure, upgrade_path_available,
    upgrade_path_request,
};
use super::io::{receive_plain, send_plain};
use quickshare_wire::connections::{
    OfflineFrame, bandwidth_upgrade_negotiation_frame::EventType, v1_frame,
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Connection methods split handshake, transfer, and upgrade"
)]
impl Connection {
    /// Returns the medium currently carrying this connection.
    #[must_use]
    pub const fn medium(&self) -> Medium {
        self.medium
    }

    /// Returns the local bandwidth-upgrade negotiation state.
    #[must_use]
    pub const fn upgrade_state(&self) -> UpgradeState {
        self.upgrade
    }

    /// Offers a higher-bandwidth medium to the peer.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when encryption or transmission fails.
    pub fn propose_upgrade(&mut self, medium: Medium) -> Result<(), Error> {
        self.propose_upgrade_path(medium, &UpgradeCredentials::default())
    }

    /// Offers a higher-bandwidth medium and its credentials to the peer.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when encryption or transmission fails.
    pub fn propose_upgrade_path(
        &mut self,
        medium: Medium,
        credentials: &UpgradeCredentials,
    ) -> Result<(), Error> {
        self.send(&upgrade_path_available(medium.to_wire(), credentials))?;
        self.upgrade = UpgradeState::Offered(medium);
        self.upgrade_host = true;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "offer",
            outcome = "locally_written",
            event_type = "upgrade_path_available",
            "upgrade transition"
        );
        Ok(())
    }

    /// Asks the peer to advertise a higher-bandwidth path.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when encryption or transmission fails.
    pub fn request_upgrade_path(
        &mut self,
        mediums: &[Medium],
    ) -> Result<(), Error> {
        let encoded: Vec<i32> =
            mediums.iter().copied().map(Medium::to_wire).collect();
        self.send(&upgrade_path_request(&encoded))?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "request",
            outcome = "locally_written",
            event_type = "upgrade_path_request",
            "upgrade transition"
        );
        Ok(())
    }

    /// Makes `event` the next value returned by [`Self::receive`].
    pub fn unread(&mut self, event: Event) {
        self.pending_events.push_front(event);
    }

    /// Records that `medium` now carries the connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when `medium` cannot replace the current path.
    pub const fn complete_upgrade(
        &mut self,
        medium: Medium,
    ) -> Result<(), Error> {
        self.medium = medium;
        self.upgrade = UpgradeState::Idle;
        self.upgrade_host = false;
        Ok(())
    }

    /// Continues this session on `stream` after the standard upgrade handshake.
    ///
    /// The prior stream is kept until last-write and safe-to-close complete.
    /// Payload and sequence state stay on this connection.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when the new channel handshake fails.
    pub fn complete_upgrade_io<Stream>(
        &mut self,
        medium: Medium,
        stream: Stream,
    ) -> Result<(), Error>
    where
        Stream: ConnectionIo + 'static,
    {
        let mut new_stream = Box::new(stream);
        self.upgrade = UpgradeState::Accepted(medium);
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "handshake",
            outcome = "started",
            "upgrade transition"
        );
        if self.upgrade_host {
            self.complete_upgrade_as_host(&mut *new_stream)?;
        } else {
            self.complete_upgrade_as_client(&mut *new_stream)?;
        }
        self.stream = new_stream;
        self.complete_upgrade(medium)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "switch_medium",
            outcome = "completed",
            "upgrade transition"
        );
        Ok(())
    }

    fn complete_upgrade_as_host(
        &mut self,
        new_stream: &mut dyn ConnectionIo,
    ) -> Result<(), Error> {
        Self::expect_plain_upgrade(
            new_stream,
            EventType::ClientIntroduction as i32,
        )?;
        send_plain(new_stream, &client_introduction_ack())?;
        upgrade_sent("client_introduction_ack");
        self.send(&last_write_to_prior_channel())?;
        upgrade_sent("last_write_to_prior_channel");
        self.expect_current_upgrade(EventType::LastWriteToPriorChannel as i32)?;
        self.send(&safe_to_close_prior_channel())?;
        upgrade_sent("safe_to_close_prior_channel");
        self.expect_current_upgrade(EventType::SafeToClosePriorChannel as i32)
    }

    fn complete_upgrade_as_client(
        &mut self,
        new_stream: &mut dyn ConnectionIo,
    ) -> Result<(), Error> {
        let endpoint_id = self.endpoint_id.clone();
        send_plain(new_stream, &client_introduction(&endpoint_id))?;
        upgrade_sent("client_introduction");
        Self::expect_plain_upgrade(
            new_stream,
            EventType::ClientIntroductionAck as i32,
        )?;
        self.send(&last_write_to_prior_channel())?;
        upgrade_sent("last_write_to_prior_channel");
        self.expect_current_upgrade(EventType::LastWriteToPriorChannel as i32)?;
        self.expect_current_upgrade(EventType::SafeToClosePriorChannel as i32)?;
        self.send(&safe_to_close_prior_channel())?;
        upgrade_sent("safe_to_close_prior_channel");
        Ok(())
    }

    /// Records that `attempted` failed and payload stays on the current medium.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when encryption or transmission fails.
    pub fn fail_upgrade(&mut self, attempted: Medium) -> Result<(), Error> {
        self.send(&upgrade_failure(attempted.to_wire()))?;
        self.upgrade = UpgradeState::Failed {
            attempted,
            fallback: self.medium,
        };
        self.upgrade_host = false;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "fallback",
            outcome = "locally_written",
            event_type = "upgrade_failure",
            "upgrade transition"
        );
        Ok(())
    }

    fn expect_plain_upgrade(
        stream: &mut dyn ConnectionIo,
        event_type: i32,
    ) -> Result<(), Error> {
        let frame = receive_plain(stream)?;
        Self::upgrade_event_type(frame, event_type)
    }
    fn expect_current_upgrade(&mut self, event_type: i32) -> Result<(), Error> {
        const MAX_QUEUED_FRAMES: usize = 64;
        let mut queued_frames = 0_usize;
        let mut queued_bytes = 0_usize;
        loop {
            let frame = self.recv()?;
            if is_upgrade_event_type(&frame, event_type) {
                return Self::upgrade_event_type(frame, event_type);
            }
            queued_frames = queued_frames.saturating_add(1);
            queued_bytes =
                queued_bytes.saturating_add(payload_body_len(&frame));
            if queued_frames > MAX_QUEUED_FRAMES
                || queued_bytes > MAX_FRAME_LENGTH
            {
                upgrade_rejected("interleaved_frame_budget");
                return Err(Error::InvalidPayload);
            }
            let insertion = self.pending_events.len();
            match self.event(frame)? {
                Some(Event::Disconnected) => return Err(Error::Rejected),
                Some(
                    event @ Event::Upgrade {
                        event: UpgradeEvent::Failure { .. },
                    },
                ) => {
                    self.pending_events.insert(insertion, event);
                    return Err(Error::Rejected);
                }
                Some(event) => self.pending_events.insert(insertion, event),
                None => {}
            }
        }
    }
    fn upgrade_event_type(
        frame: OfflineFrame,
        event_type: i32,
    ) -> Result<(), Error> {
        let Some(v1) = frame.v1 else {
            upgrade_rejected("missing_v1");
            return Err(Error::UnexpectedFrame);
        };
        if v1.r#type
            != Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32)
        {
            upgrade_rejected("unexpected_frame_type");
            return Err(Error::UnexpectedFrame);
        }
        let negotiation =
            v1.bandwidth_upgrade_negotiation.ok_or_else(|| {
                upgrade_rejected("missing_negotiation");
                Error::UnexpectedFrame
            })?;
        if negotiation.event_type != Some(event_type) {
            upgrade_rejected("unexpected_event_type");
            return Err(Error::UnexpectedFrame);
        }
        let event_name = upgrade_event_name(event_type);
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "upgrade",
            operation = "receive",
            outcome = "completed",
            event_type = event_name,
            "upgrade transition"
        );
        Ok(())
    }
}

fn payload_body_len(frame: &OfflineFrame) -> usize {
    frame
        .v1
        .as_ref()
        .and_then(|v1| v1.payload_transfer.as_ref())
        .and_then(|transfer| transfer.payload_chunk.as_ref())
        .and_then(|chunk| chunk.body.as_ref())
        .map_or(0, Vec::len)
}

fn is_upgrade_event_type(frame: &OfflineFrame, event_type: i32) -> bool {
    frame.v1.as_ref().is_some_and(|v1| {
        v1.r#type
            == Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32)
            && v1.bandwidth_upgrade_negotiation.as_ref().is_some_and(
                |negotiation| negotiation.event_type == Some(event_type),
            )
    })
}

fn upgrade_rejected(reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "upgrade",
        operation = "receive",
        outcome = "rejected",
        reason,
        "upgrade frame rejected"
    );
}

fn upgrade_sent(event_type: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "upgrade",
        operation = "send",
        outcome = "locally_written",
        event_type,
        "upgrade transition"
    );
}

const fn upgrade_event_name(event_type: i32) -> &'static str {
    if event_type == EventType::ClientIntroduction as i32 {
        "client_introduction"
    } else if event_type == EventType::ClientIntroductionAck as i32 {
        "client_introduction_ack"
    } else if event_type == EventType::LastWriteToPriorChannel as i32 {
        "last_write_to_prior_channel"
    } else if event_type == EventType::SafeToClosePriorChannel as i32 {
        "safe_to_close_prior_channel"
    } else {
        "unknown"
    }
}
