use super::{
    IncomingOffer, OfferKind, ProtocolError, SharingSession, frames, offer,
};
use quickshare_connections::Event;
use quickshare_wire::sharing::text_metadata;

const TEXT_PAYLOAD_ID: i64 = 3;

impl SharingSession {
    /// Introduces one plain-text value, reports peer consent, then sends it.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid lengths, peer rejection, or transfer
    /// failure.
    pub fn send_outgoing_text<Accepted, Progress, Cancelled>(
        &mut self,
        value: &str,
        on_accepted: Accepted,
        on_progress: Progress,
        is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Accepted: FnOnce(),
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.send_outgoing_value(
            value,
            text_metadata::Type::Text,
            on_accepted,
            on_progress,
            is_cancelled,
        )
    }

    /// Introduces one URL, reports peer consent, then sends it.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid lengths, peer rejection, or transfer
    /// failure.
    pub fn send_outgoing_url<Accepted, Progress, Cancelled>(
        &mut self,
        value: &str,
        on_accepted: Accepted,
        on_progress: Progress,
        is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Accepted: FnOnce(),
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.send_outgoing_value(
            value,
            text_metadata::Type::Url,
            on_accepted,
            on_progress,
            is_cancelled,
        )
    }

    /// Receives the BYTES payload for an accepted inbound text offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload kind, identifier, length, or UTF-8
    /// encoding does not match the introduction.
    pub fn receive_incoming_text<Progress, Cancelled>(
        &mut self,
        offer: &IncomingOffer,
        on_progress: Progress,
        is_cancelled: Cancelled,
    ) -> Result<String, ProtocolError>
    where
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.receive_incoming_value(
            offer,
            OfferKind::Text,
            on_progress,
            is_cancelled,
        )
    }

    /// Receives the BYTES payload for an accepted inbound URL offer.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload kind, identifier, length, or UTF-8
    /// encoding does not match the introduction.
    pub fn receive_incoming_url<Progress, Cancelled>(
        &mut self,
        offer: &IncomingOffer,
        on_progress: Progress,
        is_cancelled: Cancelled,
    ) -> Result<String, ProtocolError>
    where
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.receive_incoming_value(
            offer,
            OfferKind::Url,
            on_progress,
            is_cancelled,
        )
    }

    fn send_outgoing_value<Accepted, Progress, Cancelled>(
        &mut self,
        value: &str,
        kind: text_metadata::Type,
        on_accepted: Accepted,
        mut on_progress: Progress,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Accepted: FnOnce(),
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        if value.is_empty() {
            return Err(ProtocolError::InvalidOffer("empty text value"));
        }
        let wire_size = i64::try_from(value.len())
            .map_err(|_| ProtocolError::InvalidPayload)?;
        if wire_size > offer::max_text_bytes() {
            return Err(ProtocolError::InvalidPayload);
        }
        if is_cancelled() {
            return Err(ProtocolError::Cancelled);
        }
        self.send_control_frame(&frames::text_introduction(
            value, wire_size, kind,
        ))?;
        frames::consent_result(Self::decode_response(&self.receive_bytes()?)?)?;
        on_accepted();
        self.stop_if_cancelled(&mut is_cancelled)?;
        self.connection
            .send_bytes(TEXT_PAYLOAD_ID, value.as_bytes())?;
        on_progress(u64::try_from(value.len()).unwrap_or(0));
        self.wait_for_outgoing_completion(TEXT_PAYLOAD_ID, &mut is_cancelled)
    }

    fn receive_incoming_value<Progress, Cancelled>(
        &mut self,
        offer: &IncomingOffer,
        expected: OfferKind,
        mut on_progress: Progress,
        mut is_cancelled: Cancelled,
    ) -> Result<String, ProtocolError>
    where
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        if offer.kind() != expected {
            return Err(ProtocolError::InvalidPayload);
        }
        loop {
            self.stop_if_cancelled(&mut is_cancelled)?;
            match self.next_transfer_event_for(offer.payload_id())? {
                Event::Bytes { id, bytes } if id == offer.payload_id() => {
                    let size = i64::try_from(bytes.len())
                        .map_err(|_| ProtocolError::InvalidPayload)?;
                    if size != offer.size_bytes() {
                        return Err(ProtocolError::InvalidPayload);
                    }
                    on_progress(u64::try_from(bytes.len()).unwrap_or(0));
                    return String::from_utf8(bytes)
                        .map_err(|_| ProtocolError::InvalidPayload);
                }
                Event::Bytes { bytes, .. } if frames::is_v1_control(&bytes) => {
                    if frames::is_cancel(&bytes)? {
                        return Err(ProtocolError::Cancelled);
                    }
                }
                Event::Disconnected => {
                    return Err(ProtocolError::Disconnected);
                }
                _ => return Err(ProtocolError::InvalidPayload),
            }
        }
    }
}
