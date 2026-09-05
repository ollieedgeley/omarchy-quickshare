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
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "offer",
                outcome = "rejected",
                reason = "empty_text",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidOffer("empty text value"));
        }
        let wire_size = i64::try_from(value.len()).map_err(|_| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "payload",
                outcome = "rejected",
                reason = "size_out_of_range",
                "protocol_stage"
            );
            ProtocolError::InvalidPayload
        })?;
        if wire_size > offer::max_text_bytes() {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "payload",
                outcome = "rejected",
                reason = "size_exceeded",
                byte_count = wire_size,
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidPayload);
        }
        if is_cancelled() {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "control",
                operation = "cancel",
                outcome = "cancelled",
                reason = "local",
                "protocol_stage"
            );
            return Err(ProtocolError::Cancelled);
        }
        let event_type = if kind == text_metadata::Type::Text {
            "text"
        } else {
            "url"
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "introduction",
            operation = "send",
            outcome = "started",
            event_type,
            byte_count = wire_size,
            "protocol_stage"
        );
        self.send_control_frame(&frames::text_introduction(
            value, wire_size, kind,
        ))
        .inspect_err(|_error| {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "introduction",
                operation = "send",
                outcome = "failed",
                event_type,
                "protocol_stage"
            );
        })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "introduction",
            operation = "send",
            outcome = "locally_written",
            frame_type = "introduction",
            event_type,
            byte_count = wire_size,
            "protocol_stage"
        );
        frames::consent_result(Self::decode_response(&self.receive_bytes()?)?)?;
        on_accepted();
        self.stop_if_cancelled(&mut is_cancelled)?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "send",
            outcome = "started",
            frame_type = "bytes",
            byte_count = wire_size,
            "protocol_stage"
        );
        self.connection
            .send_bytes(TEXT_PAYLOAD_ID, value.as_bytes())
            .map_err(|error| {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "transfer",
                    operation = "send",
                    outcome = "failed",
                    frame_type = "bytes",
                    "protocol_stage"
                );
                ProtocolError::from(error)
            })?;
        on_progress(u64::try_from(value.len()).unwrap_or(0));
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "terminal_marker",
            outcome = "locally_written",
            frame_type = "bytes",
            byte_count = wire_size,
            "protocol_stage"
        );
        self.stop_if_cancelled(&mut is_cancelled)
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
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "validation",
                operation = "payload",
                outcome = "rejected",
                reason = "attachment_type_mismatch",
                "protocol_stage"
            );
            return Err(ProtocolError::InvalidPayload);
        }
        let event_type = match expected {
            OfferKind::Text => "text",
            OfferKind::Url => "url",
            OfferKind::File | OfferKind::AndroidApp => "file",
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "receive",
            outcome = "started",
            event_type,
            byte_count = offer.size_bytes(),
            "protocol_stage"
        );
        let mut skipped_control_reported = false;
        loop {
            self.stop_if_cancelled(&mut is_cancelled)?;
            let event = self
                .next_transfer_event_for(offer.payload_id())
                .inspect_err(|_error| {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "receive",
                        outcome = "failed",
                        event_type,
                        "protocol_stage"
                    );
                })?;
            match event {
                Event::Bytes { id, bytes } if id == offer.payload_id() => {
                    let size = i64::try_from(bytes.len()).map_err(|_| {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "validation",
                            operation = "payload",
                            outcome = "rejected",
                            reason = "size_out_of_range",
                            "protocol_stage"
                        );
                        ProtocolError::InvalidPayload
                    })?;
                    if size != offer.size_bytes() {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "validation",
                            operation = "payload",
                            outcome = "rejected",
                            reason = "size_mismatch",
                            id_matches_expected = true,
                            size_matches_expected = false,
                            byte_count = size,
                            "protocol_stage"
                        );
                        return Err(ProtocolError::InvalidPayload);
                    }
                    let value = String::from_utf8(bytes).map_err(|_| {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "validation",
                            operation = "payload",
                            outcome = "rejected",
                            reason = "utf8_invalid",
                            "protocol_stage"
                        );
                        ProtocolError::InvalidPayload
                    })?;
                    on_progress(u64::try_from(value.len()).unwrap_or(0));
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "receive",
                        outcome = "completed",
                        event_type,
                        byte_count = size,
                        "protocol_stage"
                    );
                    return Ok(value);
                }
                Event::Bytes { bytes, .. } => {
                    if let Some(control_type) =
                        frames::control_event_type(&bytes)
                    {
                        if control_type == "cancel" {
                            tracing::debug!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "cancelled",
                                reason = "remote",
                                event_type = control_type,
                                "protocol_stage"
                            );
                            return Err(ProtocolError::Cancelled);
                        }
                        if skipped_control_reported {
                            tracing::trace!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "skipped",
                                event_type = control_type,
                                "protocol_stage"
                            );
                        } else {
                            tracing::debug!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "skipped",
                                event_type = control_type,
                                "protocol_stage"
                            );
                            skipped_control_reported = true;
                        }
                    } else {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "validation",
                            operation = "payload",
                            outcome = "rejected",
                            reason = "id_mismatch",
                            id_matches_expected = false,
                            "protocol_stage"
                        );
                        return Err(ProtocolError::InvalidPayload);
                    }
                }
                Event::Disconnected => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "receive",
                        outcome = "failed",
                        reason = "connection_disconnected",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::Disconnected);
                }
                _ => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "unexpected_event",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::InvalidPayload);
                }
            }
        }
    }
}
