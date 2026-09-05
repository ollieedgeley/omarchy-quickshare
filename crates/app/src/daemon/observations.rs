//! Daemon-owned share observations for control JSON.

use core::sync::atomic::{AtomicU64, Ordering};
use core::time::Duration;
use std::io;

use tracing::{Span, field};

use quickshare_sharing::{PairingError, PairingStatus, ProtocolError};
use quickshare_storage::Error as StorageError;

/// LAN TCP medium name exposed on the public snapshot.
pub(super) const WIFI_LAN: &str = "wifi_lan";
/// Wi-Fi hotspot medium name.
pub(super) const WIFI_HOTSPOT: &str = "wifi_hotspot";
/// Wi-Fi Direct medium name.
pub(super) const WIFI_DIRECT: &str = "wifi_direct";
/// BLE medium name.
pub(super) const BLE: &str = "ble";
/// Bluetooth Classic medium name.
pub(super) const BLUETOOTH: &str = "bluetooth";

static NEXT_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

/// Opens one process-local chronology before the protocol handshake starts.
pub(super) fn connection_span(
    direction: &'static str,
    initial_medium: &'static str,
    share_id: Option<u64>,
) -> Span {
    let connection_id = NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
    let span = tracing::debug_span!(
        target: "omarchy_quickshare::protocol",
        "connection",
        connection_id,
        direction,
        initial_medium,
        share_id = field::Empty
    );
    if let Some(share_id) = share_id {
        let _recorded = span.record("share_id", share_id);
    }
    span
}

/// Records the stable local share identifier once inbound consent supplies it.
pub(super) fn record_share_id(span: &Span, share_id: u64) {
    let _recorded = span.record("share_id", share_id);
}

/// Maps an operating-system I/O failure to a privacy-safe stable class.
#[must_use]
pub(super) fn io_error_kind(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::ConnectionRefused => "connection_refused",
        io::ErrorKind::ConnectionReset => "connection_reset",
        io::ErrorKind::ConnectionAborted => "connection_aborted",
        io::ErrorKind::NotConnected => "not_connected",
        io::ErrorKind::AddrInUse => "address_in_use",
        io::ErrorKind::AddrNotAvailable => "address_unavailable",
        io::ErrorKind::BrokenPipe => "broken_pipe",
        io::ErrorKind::AlreadyExists => "already_exists",
        io::ErrorKind::WouldBlock => "would_block",
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::InvalidData => "invalid_data",
        io::ErrorKind::TimedOut => "timed_out",
        io::ErrorKind::WriteZero => "write_zero",
        io::ErrorKind::UnexpectedEof => "unexpected_eof",
        io::ErrorKind::OutOfMemory => "out_of_memory",
        _ => "other",
    }
}

/// Returns the underlying I/O class when a protocol error preserves it.
#[must_use]
pub(super) fn protocol_io_kind(error: &ProtocolError) -> Option<&'static str> {
    match error {
        ProtocolError::Io(source) => Some(io_error_kind(source)),
        ProtocolError::Connection(quickshare_connections::Error::Io(
            source,
        )) => Some(io_error_kind(source)),
        _ => None,
    }
}

/// Emits one safe protocol-orchestration result inside the active chronology.
pub(super) fn trace_protocol(
    stage: &'static str,
    operation: &'static str,
    outcome: &'static str,
    reason: Option<&str>,
    io_error_kind: Option<&str>,
) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage,
        operation,
        outcome,
        reason = reason.unwrap_or("none"),
        io_error_kind = io_error_kind.unwrap_or("none"),
        "protocol stage"
    );
}

/// Estimates remaining seconds from observed throughput.
#[must_use]
pub(super) fn remaining_seconds(
    transferred: u64,
    total: u64,
    elapsed: Duration,
) -> Option<u64> {
    let elapsed_secs = elapsed.as_secs();
    if transferred == 0 || elapsed_secs == 0 || transferred > total {
        return None;
    }
    Some(
        total
            .saturating_sub(transferred)
            .saturating_mul(elapsed_secs)
            / transferred,
    )
}

/// Emits the safe result of one paired-key exchange.
pub(super) fn trace_paired_key_exchange(
    result: &Result<PairingStatus, PairingError>,
    direction: &'static str,
    medium: &str,
    share_id: Option<u64>,
) {
    match result {
        Ok(_) => tracing::debug!(
            target: "omarchy_quickshare::protocol",
            share_id,
            stage = "paired_key",
            direction,
            medium,
            outcome = "completed",
            "paired-key exchange completed"
        ),
        Err(error) => tracing::warn!(
            target: "omarchy_quickshare::protocol",
            share_id,
            stage = "paired_key",
            direction,
            medium,
            pairing_step = error.step().as_str(),
            error_class = error.source_error().reason(),
            outcome = "failed",
            "paired-key exchange failed"
        ),
    }
}

/// Maps a storage failure to a stable snake_case reason.
#[must_use]
pub(super) const fn storage_reason(error: &StorageError) -> &'static str {
    match error {
        StorageError::Collision => "collision",
        StorageError::Interrupted => "interrupted",
        StorageError::InvalidName => "invalid_name",
        StorageError::InvalidSource => "invalid_source",
        StorageError::Mutation => "mutation",
        StorageError::Quota => "quota",
        StorageError::SizeMismatch => "size_mismatch",
        StorageError::Io(_) => "io",
        _ => "failed",
    }
}

/// Emits one privacy-safe storage orchestration boundary.
pub(super) fn trace_storage(
    operation: &'static str,
    outcome: &'static str,
    error: Option<&StorageError>,
) {
    let reason = error.map(storage_reason);
    let io_error_kind = error.and_then(|error| match error {
        StorageError::Io(source) => Some(io_error_kind(source)),
        _ => None,
    });
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "storage",
        operation,
        outcome,
        reason = reason.unwrap_or("none"),
        io_error_kind = io_error_kind.unwrap_or("none"),
        "storage stage"
    );
}

/// Returns recovery guidance for a public terminal reason.
#[must_use]
pub(super) fn recovery_guidance(reason: &str) -> &'static str {
    match reason {
        "cancelled" => "Submit the content again if you still want to send it.",
        "collision" => {
            "Rename the inbound file or clear the receive directory."
        }
        "disconnected" => "Reconnect and send the share again.",
        "interrupted" => "Accept the share again to finish receiving.",
        "invalid_name" | "invalid_payload" | "unsupported" => {
            "Send a file, text, or URL instead."
        }
        "mutation" => {
            "Keep the outbound file unchanged until the share completes."
        }
        "quota" => "Free space in the receive directory and retry.",
        "rejected" => "Choose another peer or ask the receiver to accept.",
        "size_mismatch" => "Retry the share; the file size did not match.",
        "timed_out" => "Retry while both devices stay nearby.",
        _ => "Retry the share. Confirm the peer is nearby.",
    }
}

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "Observation contracts stay beside the mapping functions"
)]
mod tests {
    use super::{
        WIFI_LAN, recovery_guidance, remaining_seconds, storage_reason,
    };
    use core::time::Duration;
    use quickshare_storage::Error as StorageError;

    #[test]
    fn eta_uses_observed_throughput() {
        assert_eq!(
            remaining_seconds(50, 100, Duration::from_millis(1000)),
            Some(1)
        );
        assert_eq!(remaining_seconds(0, 100, Duration::from_secs(1)), None);
    }

    #[test]
    fn storage_reasons_are_stable_snake_case() {
        assert_eq!(storage_reason(&StorageError::Collision), "collision");
        assert_eq!(storage_reason(&StorageError::Quota), "quota");
        assert_eq!(storage_reason(&StorageError::Mutation), "mutation");
        assert_eq!(
            storage_reason(&StorageError::SizeMismatch),
            "size_mismatch"
        );
        assert_eq!(storage_reason(&StorageError::Interrupted), "interrupted");
        assert_eq!(WIFI_LAN, "wifi_lan");
    }

    #[test]
    fn recovery_guidance_avoids_peer_and_file_names() {
        for reason in
            ["timed_out", "disconnected", "quota", "collision", "failed"]
        {
            let guidance = recovery_guidance(reason);
            assert!(!guidance.contains('/'));
            assert!(!guidance.to_ascii_lowercase().contains("pixel"));
        }
    }
}
