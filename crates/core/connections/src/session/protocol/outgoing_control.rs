use crate::{Connection, Error, Event};
use core::time::Duration;
use std::{io, time::Instant};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Connection methods remain grouped by protocol operation"
)]
impl Connection {
    /// Receives at most one event without waiting for an idle stream.
    ///
    /// Once an encrypted frame has started, its existing framing and payload
    /// rules apply under a bounded read deadline. Multi-frame payloads return
    /// [`None`] until their normal reassembly completes.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] when a ready frame is malformed, unauthenticated, or
    /// cannot be read before the bounded deadline.
    pub fn poll_event(&mut self) -> Result<Option<Event>, Error> {
        if let Some(event) = self.pending_events.pop_front() {
            return Ok(Some(event));
        }
        let deadline = self.immediate_read_deadline()?;
        let frame = match self.recv_if_ready(deadline) {
            Err(Error::Io(error))
                if error.kind() == io::ErrorKind::UnexpectedEof =>
            {
                return Ok(Some(Event::Disconnected));
            }
            other => other?,
        };
        frame.map_or(Ok(None), |ready_frame| self.event(ready_frame))
    }

    fn immediate_read_deadline(&self) -> Result<Instant, Error> {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(1))
            .ok_or_else(|| {
                Error::Io(io::Error::other("control poll overflow"))
            })?;
        Ok(self
            .read_deadline
            .map_or(deadline, |value| value.min(deadline)))
    }

    pub(super) fn poll_outgoing_control(
        &mut self,
        payload_id: i64,
    ) -> Result<(), Error> {
        let deadline = self.immediate_read_deadline()?;
        loop {
            let frame = match self.recv_if_ready(deadline) {
                Err(Error::Io(error))
                    if error.kind() == io::ErrorKind::UnexpectedEof =>
                {
                    self.outgoing_file = None;
                    return Err(Error::Rejected);
                }
                other => other?,
            };
            let Some(ready_frame) = frame else {
                return Ok(());
            };
            match self.event(ready_frame)? {
                Some(Event::PayloadCancelled { id, .. })
                    if id == payload_id =>
                {
                    self.outgoing_file = None;
                    return Err(Error::Cancelled);
                }
                Some(Event::PayloadError { id, .. }) if id == payload_id => {
                    self.outgoing_file = None;
                    return Err(Error::InvalidPayload);
                }
                Some(
                    Event::PayloadCancelled { .. }
                    | Event::PayloadError { .. }
                    | Event::KeepAlive { .. },
                )
                | None => {}
                Some(Event::Disconnected) => {
                    self.outgoing_file = None;
                    return Err(Error::Rejected);
                }
                Some(event) => self.pending_events.push_back(event),
            }
        }
    }
}
