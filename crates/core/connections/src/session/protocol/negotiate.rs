use super::super::{
    Connection, ConnectionIo, Error, Event, Medium, UpgradeCredentials,
    UpgradeState,
};
use super::frames::{
    client_introduction, client_introduction_ack, last_write_to_prior_channel,
    safe_to_close_prior_channel, upgrade_failure, upgrade_path_available,
    upgrade_path_request,
};
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
        self.send(&upgrade_path_request(&encoded))
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
        if self.upgrade_host {
            self.expect_upgrade(
                &mut *new_stream,
                EventType::ClientIntroduction as i32,
            )?;
            self.send_on(&mut *new_stream, &client_introduction_ack())?;
            self.send(&last_write_to_prior_channel())?;
            self.expect_current_upgrade(
                EventType::LastWriteToPriorChannel as i32,
            )?;
            self.send_on(&mut *new_stream, &safe_to_close_prior_channel())?;
            self.expect_upgrade(
                &mut *new_stream,
                EventType::SafeToClosePriorChannel as i32,
            )?;
        } else {
            let endpoint_id = self.endpoint_id.clone();
            self.send_on(&mut *new_stream, &client_introduction(&endpoint_id))?;
            self.expect_upgrade(
                &mut *new_stream,
                EventType::ClientIntroductionAck as i32,
            )?;
            self.expect_current_upgrade(
                EventType::LastWriteToPriorChannel as i32,
            )?;
            self.send(&last_write_to_prior_channel())?;
            self.expect_upgrade(
                &mut *new_stream,
                EventType::SafeToClosePriorChannel as i32,
            )?;
            self.send_on(&mut *new_stream, &safe_to_close_prior_channel())?;
        }
        self.stream = new_stream;
        self.complete_upgrade(medium)
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
        Ok(())
    }

    fn expect_upgrade(
        &mut self,
        stream: &mut dyn ConnectionIo,
        event_type: i32,
    ) -> Result<(), Error> {
        let frame = self.recv_on(stream)?;
        Self::upgrade_event_type(frame, event_type)
    }
    fn expect_current_upgrade(&mut self, event_type: i32) -> Result<(), Error> {
        let frame = self.recv()?;
        Self::upgrade_event_type(frame, event_type)
    }
    fn upgrade_event_type(
        frame: OfflineFrame,
        event_type: i32,
    ) -> Result<(), Error> {
        let v1 = frame.v1.ok_or(Error::UnexpectedFrame)?;
        if v1.r#type
            != Some(v1_frame::FrameType::BandwidthUpgradeNegotiation as i32)
        {
            return Err(Error::UnexpectedFrame);
        }
        let negotiation = v1
            .bandwidth_upgrade_negotiation
            .ok_or(Error::UnexpectedFrame)?;
        if negotiation.event_type != Some(event_type) {
            return Err(Error::UnexpectedFrame);
        }
        Ok(())
    }
}
