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

/// Maps a Sharing protocol failure to a stable snake_case reason.
#[must_use]
pub(super) const fn protocol_reason(error: &ProtocolError) -> &'static str {
    match error {
        ProtocolError::Cancelled => "cancelled",
        ProtocolError::Disconnected | ProtocolError::Connection(_) => {
            "disconnected"
        }
        ProtocolError::Rejected => "rejected",
        ProtocolError::TimedOut => "timed_out",
        ProtocolError::Unsupported => "unsupported",
        ProtocolError::Io(_) => "io",
        ProtocolError::InvalidOffer(_) | ProtocolError::InvalidPayload => {
            "invalid_payload"
        }
        _ => "failed",
    }
}

/// Maps a paired-key failure to its safe protocol discriminant.
#[must_use]
pub(super) const fn pairing_error_class(error: &ProtocolError) -> &'static str {
    match error {
        ProtocolError::Connection(source) => match source {
            quickshare_connections::Error::Io(_) => "connection_io",
            quickshare_connections::Error::Wire(_) => "connection_wire",
            quickshare_connections::Error::FrameTooLarge => {
                "connection_frame_too_large"
            }
            quickshare_connections::Error::UnexpectedFrame => {
                "connection_unexpected_frame"
            }
            quickshare_connections::Error::Rejected => "connection_rejected",
            quickshare_connections::Error::Handshake => "connection_handshake",
            quickshare_connections::Error::Crypto => "connection_crypto",
            quickshare_connections::Error::InvalidPayload => {
                "connection_invalid_payload"
            }
            _ => "connection_unknown",
        },
        ProtocolError::Decode(_) => "sharing_decode",
        ProtocolError::Io(_) => "sharing_io",
        ProtocolError::Cancelled => "cancelled",
        ProtocolError::InvalidAdvertisement => "invalid_advertisement",
        ProtocolError::InvalidMdnsInstance => "invalid_mdns_instance",
        ProtocolError::InvalidFrame => "invalid_frame",
        ProtocolError::InvalidOffer(_) => "invalid_offer",
        ProtocolError::Rejected => "rejected",
        ProtocolError::InvalidPayload => "invalid_payload",
        ProtocolError::Disconnected => "disconnected",
        ProtocolError::TimedOut => "timed_out",
        ProtocolError::Unsupported => "unsupported",
    }
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
            error_class = pairing_error_class(error.source_error()),
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
        WIFI_LAN, pairing_error_class, protocol_reason, recovery_guidance,
        remaining_seconds, storage_reason,
    };
    use core::time::Duration;
    use prost::Message as _;
    use quickshare_connections::Error as ConnectionError;
    use quickshare_sharing::ProtocolError;
    use quickshare_storage::Error as StorageError;
    use std::io;

    #[derive(Clone, PartialEq, prost::Message)]
    struct DecodeProbe {
        #[prost(uint32, tag = "1")]
        value: u32,
    }

    #[expect(
        clippy::panic,
        reason = "A fixed malformed protobuf must not decode"
    )]
    fn decode_error() -> prost::DecodeError {
        let Err(error) = DecodeProbe::decode(&[0x80_u8][..]) else {
            panic!("malformed protobuf decoded");
        };
        error
    }

    #[test]
    fn eta_uses_observed_throughput() {
        assert_eq!(
            remaining_seconds(50, 100, Duration::from_millis(1000)),
            Some(1)
        );
        assert_eq!(remaining_seconds(0, 100, Duration::from_secs(1)), None);
    }

    #[test]
    fn protocol_and_storage_reasons_are_stable_snake_case() {
        assert_eq!(protocol_reason(&ProtocolError::TimedOut), "timed_out");
        assert_eq!(
            protocol_reason(&ProtocolError::Disconnected),
            "disconnected"
        );
        assert_eq!(protocol_reason(&ProtocolError::Unsupported), "unsupported");
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
    fn paired_key_error_classes_identify_every_protocol_discriminant() {
        let cases = [
            (ProtocolError::Decode(decode_error()), "sharing_decode"),
            (ProtocolError::Io(io::Error::other("test")), "sharing_io"),
            (ProtocolError::Cancelled, "cancelled"),
            (ProtocolError::InvalidAdvertisement, "invalid_advertisement"),
            (ProtocolError::InvalidMdnsInstance, "invalid_mdns_instance"),
            (ProtocolError::InvalidFrame, "invalid_frame"),
            (ProtocolError::InvalidOffer("test"), "invalid_offer"),
            (ProtocolError::Rejected, "rejected"),
            (ProtocolError::InvalidPayload, "invalid_payload"),
            (ProtocolError::Disconnected, "disconnected"),
            (ProtocolError::TimedOut, "timed_out"),
            (ProtocolError::Unsupported, "unsupported"),
        ];

        for (error, expected) in cases {
            assert_eq!(pairing_error_class(&error), expected);
        }
    }

    #[test]
    fn paired_key_connection_classes_preserve_the_public_reason() {
        let cases = [
            (
                ConnectionError::Io(io::Error::other("test")),
                "connection_io",
            ),
            (ConnectionError::Wire(decode_error()), "connection_wire"),
            (ConnectionError::FrameTooLarge, "connection_frame_too_large"),
            (
                ConnectionError::UnexpectedFrame,
                "connection_unexpected_frame",
            ),
            (ConnectionError::Rejected, "connection_rejected"),
            (ConnectionError::Handshake, "connection_handshake"),
            (ConnectionError::Crypto, "connection_crypto"),
            (
                ConnectionError::InvalidPayload,
                "connection_invalid_payload",
            ),
        ];

        for (source, expected) in cases {
            let error = ProtocolError::Connection(source);
            assert_eq!(pairing_error_class(&error), expected);
            assert_eq!(protocol_reason(&error), "disconnected");
        }
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
