use super::SharingSession;
use crate::protocol::{ProtocolError, frames};
use core::time::Duration;
use quickshare_connections::{Error as ConnectionError, Event};
use std::io;

const OUTGOING_COMPLETION_TIMEOUT: Duration = Duration::from_secs(60);

impl SharingSession {
    pub(in crate::protocol) fn wait_for_outgoing_completion<Cancelled>(
        &mut self,
        payload_id: i64,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Cancelled: FnMut() -> bool,
    {
        self.stop_if_cancelled(is_cancelled)?;
        if let Err(error) = self
            .connection
            .set_read_timeout(OUTGOING_COMPLETION_TIMEOUT)
        {
            tracing::debug!(
                target: "omarchy_quickshare::protocol",
                stage = "transfer",
                operation = "completion_wait",
                outcome = "failed",
                reason = "deadline_setup",
                io_error_kind = "other",
                "protocol_stage"
            );
            return Err(ProtocolError::from(error));
        }
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "transfer",
            operation = "completion_wait",
            outcome = "deadline_started",
            "protocol_stage"
        );
        let mut skipped_control_reported = false;
        loop {
            self.stop_if_cancelled(is_cancelled)?;
            let event = match self.next_transfer_event_for(payload_id) {
                Ok(event) => event,
                Err(error) => {
                    if is_cancelled() {
                        tracing::debug!(
                            target: "omarchy_quickshare::protocol",
                            stage = "transfer",
                            operation = "completion_wait",
                            outcome = "cancelled",
                            reason = "local",
                            "protocol_stage"
                        );
                        return Err(ProtocolError::Cancelled);
                    }
                    let timed_out = matches!(
                        &error,
                        ProtocolError::Connection(ConnectionError::Io(inner))
                            if inner.kind() == io::ErrorKind::TimedOut
                    );
                    let outcome = if timed_out {
                        "deadline_elapsed"
                    } else {
                        "failed"
                    };
                    let reason = if timed_out { "timeout" } else { "receive" };
                    let io_error_kind =
                        if timed_out { "timed_out" } else { "other" };
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "completion_wait",
                        outcome,
                        reason,
                        io_error_kind,
                        "protocol_stage"
                    );
                    return Err(error);
                }
            };
            if is_cancelled() {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "transfer",
                    operation = "completion_wait",
                    outcome = "cancelled",
                    reason = "local",
                    "protocol_stage"
                );
                return Err(ProtocolError::Cancelled);
            }
            match event {
                Event::Disconnected => {
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "transfer",
                        operation = "completion_wait",
                        outcome = "completed",
                        reason = "peer_disconnected",
                        disconnect_origin = "connection_event",
                        "protocol_stage"
                    );
                    return Ok(());
                }
                Event::Bytes { bytes, .. } => {
                    if let Some(event_type) = frames::control_event_type(&bytes)
                    {
                        if event_type == "cancel" {
                            tracing::debug!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "cancelled",
                                reason = "remote",
                                event_type,
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
                                event_type,
                                "protocol_stage"
                            );
                        } else {
                            tracing::debug!(
                                target: "omarchy_quickshare::protocol",
                                stage = "control",
                                operation = "demux",
                                outcome = "skipped",
                                event_type,
                                "protocol_stage"
                            );
                            skipped_control_reported = true;
                        }
                        continue;
                    }
                    tracing::debug!(
                        target: "omarchy_quickshare::protocol",
                        stage = "control",
                        operation = "demux",
                        outcome = "rejected",
                        reason = "malformed",
                        event_type = "unknown",
                        "protocol_stage"
                    );
                    return Err(ProtocolError::InvalidPayload);
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
