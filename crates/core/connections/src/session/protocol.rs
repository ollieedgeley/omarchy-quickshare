mod frames;
mod handshake;
mod io;
mod negotiate;
mod transfer;

fn payload_trace_progress(
    operation: &'static str,
    outcome: &'static str,
    frame_type: &'static str,
    byte_count: i64,
) {
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "payload",
        operation, outcome, frame_type, byte_count, "payload progression"
    );
}
fn payload_debug_progress(
    operation: &'static str,
    outcome: &'static str,
    frame_type: &'static str,
    byte_count: i64,
) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage = "payload",
        operation, outcome, frame_type, byte_count, "payload progression"
    );
}
fn payload_debug_progress_at(
    operation: &'static str,
    outcome: &'static str,
    frame_type: &'static str,
    offset: i64,
    byte_count: usize,
) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage = "payload",
        operation, outcome, frame_type, offset, byte_count,
        "payload progression"
    );
}
fn payload_chunk_received(offset: i64, byte_count: usize) {
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "payload",
        operation = "receive", outcome = "chunk", offset, byte_count,
        "payload chunk received"
    );
}
fn payload_chunk_sent(offset: i64, byte_count: usize) {
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "payload",
        operation = "send", outcome = "chunk", frame_type = "file",
        offset, byte_count, "payload chunk sent"
    );
}
fn payload_ack_received() {
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "control",
        operation = "receive", outcome = "completed",
        frame_type = "payload_ack", "payload control dispatched"
    );
}
fn payload_control_dispatched(event_type: &'static str, offset: i64) {
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "control",
        operation = "receive", outcome = "dispatched", event_type, offset,
        "payload control dispatched"
    );
}
fn connection_event(reason: &'static str, origin: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage = "control",
        operation = "send", outcome = "locally_written", reason,
        frame_type = "disconnection", disconnect_origin = origin,
        "connection_event"
    );
}
fn connection_received() {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage = "control",
        operation = "receive", outcome = "disconnected",
        reason = "disconnect_frame", frame_type = "disconnection",
        disconnect_origin = "explicit_frame", "connection_event"
    );
}
fn keepalive_received() {
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "control",
        operation = "receive", outcome = "completed",
        frame_type = "keepalive", "keepalive received"
    );
}
fn keepalive_sent(ack: bool) {
    let frame_type = if ack { "keepalive_ack" } else { "keepalive" };
    tracing::trace!(
        target: "omarchy_quickshare::protocol", stage = "control",
        operation = "send", outcome = "locally_written", frame_type,
        "keepalive sent"
    );
}
fn frame_rejected(
    stage: &'static str,
    reason: &'static str,
    frame_type: &'static str,
) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage,
        operation = "receive", outcome = "rejected", reason, frame_type,
        "connection frame rejected"
    );
}
fn upgrade_frame_rejected(reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage = "upgrade",
        operation = "receive", outcome = "rejected", reason,
        frame_type = "bandwidth_upgrade", "upgrade frame rejected"
    );
}
fn frame_dispatch_rejected(reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol", stage = "frame_dispatch",
        operation = "receive", outcome = "rejected", reason,
        frame_type = "unknown", "connection frame rejected"
    );
}
