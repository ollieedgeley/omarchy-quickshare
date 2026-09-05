/// Records one UKEY2 step without logging peer-provided content.
macro_rules! ukey2_diagnostic {
    ($operation:expr, $role:expr, $event_type:expr, $result:expr) => {{
        let role = match $role {
            $crate::Role::Initiator => "initiator",
            $crate::Role::Responder => "responder",
        };
        match *$result {
            Ok(_) => {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "ukey2",
                    operation = $operation,
                    outcome = "completed",
                    event_type = $event_type,
                    role,
                    "UKEY2 step observed"
                );
            }
            Err(error) => {
                tracing::debug!(
                    target: "omarchy_quickshare::protocol",
                    stage = "ukey2",
                    operation = $operation,
                    outcome = "rejected",
                    reason = match error {
                        $crate::HandshakeError::InvalidMessage =>
                            "invalid_message",
                        $crate::HandshakeError::InvalidField =>
                            "invalid_field",
                        $crate::HandshakeError::InvalidState =>
                            "invalid_state",
                        $crate::HandshakeError::Unsupported =>
                            "unsupported",
                        $crate::HandshakeError::Commitment =>
                            "commitment_mismatch",
                        $crate::HandshakeError::PublicKey =>
                            "invalid_public_key",
                        $crate::HandshakeError::KeyAgreement =>
                            "key_agreement",
                    },
                    event_type = $event_type,
                    role,
                    "UKEY2 step rejected"
                );
            }
        }
    }};
}
