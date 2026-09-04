//! Daemon-owned share observations for control JSON.

use core::time::Duration;

use quickshare_sharing::ProtocolError;
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
        WIFI_LAN, protocol_reason, recovery_guidance, remaining_seconds,
        storage_reason,
    };
    use core::time::Duration;
    use quickshare_sharing::ProtocolError;
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
