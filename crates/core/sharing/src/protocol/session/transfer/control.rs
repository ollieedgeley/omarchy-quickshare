use super::SharingSession;
use crate::protocol::{ProtocolError, frames};
use core::time::Duration;
use quickshare_connections::{Error as ConnectionError, Event, UpgradeEvent};
use std::io;

impl SharingSession {
    /// Retains a completed outgoing transport for late peer control frames.
    ///
    /// # Errors
    ///
    /// Returns a typed timeout, cancellation, authentication, framing, or
    /// unexpected-payload failure after closing the transport.
    pub fn drain_post_transfer_control<Cancelled>(
        self,
        grace: Duration,
        is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Cancelled: FnMut() -> bool,
    {
        match self
            .connection
            .drain_post_transfer_control(grace, is_cancelled)
        {
            Ok(()) => Ok(()),
            Err(ConnectionError::Cancelled) => Err(ProtocolError::Cancelled),
            Err(ConnectionError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                Err(ProtocolError::TimedOut)
            }
            Err(error) => Err(ProtocolError::from(error)),
        }
    }

    /// Processes one ready peer event while local consent is pending.
    ///
    /// # Errors
    ///
    /// Returns a typed cancellation, disconnect, malformed-frame, or
    /// unexpected-payload failure.
    pub fn poll_pending_consent_control(
        &mut self,
    ) -> Result<(), ProtocolError> {
        let Some(event) = self.connection.poll_event()? else {
            return Ok(());
        };
        let (event_type, frame_type, upgrade_type) = match event {
            Event::KeepAlive { .. } => return Ok(()),
            Event::Disconnected => return Err(ProtocolError::Disconnected),
            Event::PayloadCancelled { .. } => {
                return Err(ProtocolError::Cancelled);
            }
            Event::Bytes { bytes, .. } => {
                let frame_type =
                    frames::control_event_type(&bytes).unwrap_or("unknown");
                if frame_type == "cancel" {
                    return Err(ProtocolError::Cancelled);
                }
                ("bytes", Some(frame_type), None)
            }
            Event::PayloadError { .. } => ("payload_error", None, None),
            Event::FileHeader { .. } => ("file_header", None, None),
            Event::FileChunk { .. } => ("file_chunk", None, None),
            Event::Upgrade {
                event: upgrade_event,
            } => {
                let upgrade_type = match upgrade_event {
                    UpgradeEvent::PathRequest { .. } => "path_request",
                    UpgradeEvent::PathAvailable { .. } => "path_available",
                    UpgradeEvent::LastWriteToPriorChannel => {
                        "last_write_to_prior_channel"
                    }
                    UpgradeEvent::SafeToClosePriorChannel { .. } => {
                        "safe_to_close_prior_channel"
                    }
                    UpgradeEvent::ClientIntroduction { .. } => {
                        "client_introduction"
                    }
                    UpgradeEvent::ClientIntroductionAck => {
                        "client_introduction_ack"
                    }
                    UpgradeEvent::Failure { .. } => return Ok(()),
                };
                ("upgrade", None, Some(upgrade_type))
            }
            _ => ("unknown", None, None),
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "control",
            operation = "pending_consent",
            outcome = "rejected",
            reason = "unexpected_event",
            event_type,
            frame_type,
            upgrade_type,
            "protocol_stage"
        );
        Err(ProtocolError::InvalidPayload)
    }

    pub(in crate::protocol) fn next_transfer_event_for(
        &mut self,
        payload_id: i64,
    ) -> Result<Event, ProtocolError> {
        let mut skipped_payload_control_reported = false;
        loop {
            match self.connection.receive()? {
                Event::PayloadCancelled { id, .. } if id == payload_id => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "control",
                        operation = "demux",
                        outcome = "cancelled",
                        reason = "remote",
                        event_type = "payload_cancelled",
                        id_matches_expected = true,
                        "protocol_stage"
                    );
                    return Err(ProtocolError::Cancelled);
                }
                Event::PayloadError { id, .. } if id == payload_id => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "remote_payload_error",
                        id_matches_expected = true,
                        "protocol_stage"
                    );
                    return Err(ProtocolError::InvalidPayload);
                }
                Event::KeepAlive { .. } | Event::Upgrade { .. } => {
                    tracing::trace!(
                        target: "omarchy_quickshare::protocol",
                        stage = "control",
                        operation = "demux",
                        outcome = "skipped",
                        event_type = "connection_control",
                        "protocol_stage"
                    );
                }
                event @ (Event::PayloadCancelled { .. }
                | Event::PayloadError { .. }) => {
                    let event_type =
                        if matches!(event, Event::PayloadCancelled { .. }) {
                            "payload_cancelled"
                        } else {
                            "payload_error"
                        };
                    if skipped_payload_control_reported {
                        tracing::trace!(
                            target: "omarchy_quickshare::protocol",
                            stage = "control",
                            operation = "demux",
                            outcome = "skipped",
                            event_type,
                            id_matches_expected = false,
                            "protocol_stage"
                        );
                    } else {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "control",
                            operation = "demux",
                            outcome = "skipped",
                            event_type,
                            id_matches_expected = false,
                            "protocol_stage"
                        );
                        skipped_payload_control_reported = true;
                    }
                }
                event => return Ok(event),
            }
        }
    }

    pub(in crate::protocol) fn next_transfer_event(
        &mut self,
    ) -> Result<Event, ProtocolError> {
        loop {
            match self.connection.receive()? {
                Event::KeepAlive { .. } | Event::Upgrade { .. } => {
                    tracing::trace!(
                        target: "omarchy_quickshare::protocol",
                        stage = "control",
                        operation = "demux",
                        outcome = "skipped",
                        event_type = "connection_control",
                        "protocol_stage"
                    );
                }
                Event::Disconnected => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "receive",
                        outcome = "failed",
                        reason = "connection_disconnected",
                        disconnect_origin = "connection_event",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::Disconnected);
                }
                Event::PayloadCancelled { .. } => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "control",
                        operation = "demux",
                        outcome = "cancelled",
                        reason = "remote",
                        event_type = "payload_cancelled",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::Cancelled);
                }
                Event::PayloadError { .. } => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "validation",
                        operation = "payload",
                        outcome = "rejected",
                        reason = "remote_payload_error",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::InvalidPayload);
                }
                event => return Ok(event),
            }
        }
    }

    pub(in crate::protocol) fn receive_bytes(
        &mut self,
    ) -> Result<Vec<u8>, ProtocolError> {
        match self.next_transfer_event()? {
            Event::Bytes { bytes, .. } if frames::is_cancel(&bytes)? => {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "control",
                    operation = "demux",
                    outcome = "cancelled",
                    reason = "remote",
                    event_type = "cancel",
                    "protocol_stage"
                );
                Err(ProtocolError::Cancelled)
            }
            Event::Bytes { bytes, .. } => {
                tracing::trace!(
                    target: "omarchy_quickshare::protocol",
                    stage = "transfer",
                    operation = "receive",
                    outcome = "data_arrived",
                    byte_count = bytes.len(),
                    "protocol_stage"
                );
                Ok(bytes)
            }
            _ => {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "validation",
                    operation = "control",
                    outcome = "rejected",
                    reason = "expected_bytes",
                    "protocol_stage"
                );
                Err(ProtocolError::InvalidFrame)
            }
        }
    }

    pub(in crate::protocol) fn stop_if_cancelled<Cancelled>(
        &mut self,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Cancelled: FnMut() -> bool,
    {
        if !is_cancelled() {
            return Ok(());
        }
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "control",
            operation = "cancel",
            outcome = "started",
            reason = "local",
            "protocol_stage"
        );
        let result = self.send_control_frame(&frames::cancel());
        let outcome = if result.is_ok() {
            "locally_written"
        } else {
            "failed"
        };
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "control",
            operation = "cancel",
            outcome,
            reason = "local",
            event_type = "cancel",
            "protocol_stage"
        );
        result?;
        Err(ProtocolError::Cancelled)
    }
}
