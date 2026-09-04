//! Outbound session and transfer event mapping.

use std::{io, sync::mpsc::Sender};

use quickshare_bluez::Adapter;
use quickshare_network::NetworkManager;
use quickshare_sharing::{ProtocolError, SharingSession};
use quickshare_storage::OutboundSource;

use super::{NetworkEvent, TransferCancellation, emit_progress};
use crate::daemon::media::{
    PeerRoute, attempt_order, connect_route, initiate_bandwidth_upgrade,
    medium_name, sharing_session,
};
use crate::daemon::observations::{protocol_reason, trace_paired_key_exchange};
use crate::daemon::outbound::{OutboundPayload, OutboundTransfer};

/// Converts one worker transfer attempt into a terminal daemon event.
pub(super) fn outbound_event(
    share_id: u64,
    transfer: &OutboundTransfer,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
    adapter: Option<&Adapter>,
    manager: Option<&NetworkManager>,
) -> NetworkEvent {
    let event = match send_payload(
        transfer.payload(),
        transfer.routes(),
        share_id,
        events,
        cancellation,
        adapter,
        manager,
    ) {
        Ok(bytes) => NetworkEvent::OutboundCompleted { bytes, share_id },
        Err(ProtocolError::Cancelled) => {
            NetworkEvent::OutboundCancelled { share_id }
        }
        Err(ProtocolError::Rejected) => {
            NetworkEvent::OutboundRejected { share_id }
        }
        Err(error) => NetworkEvent::OutboundFailed {
            reason: String::from(protocol_reason(&error)),
            share_id,
        },
    };
    cancellation.finish(share_id);
    event
}

fn send_payload(
    payload: &OutboundPayload,
    routes: &[PeerRoute],
    share_id: u64,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
    adapter: Option<&Adapter>,
    manager: Option<&NetworkManager>,
) -> Result<u64, ProtocolError> {
    let mut last_error = None;
    for medium in attempt_order() {
        for route in routes {
            if route.medium() != medium {
                continue;
            }
            if cancellation.is_cancelled(share_id) {
                return Err(ProtocolError::Cancelled);
            }
            let connection = match connect_route(adapter, route) {
                Ok(connection) => connection,
                Err(error) => {
                    last_error = Some(error);
                    continue;
                }
            };
            return send_on_connection(
                payload,
                connection,
                share_id,
                events,
                cancellation,
                manager,
            );
        }
    }
    Err(last_error.unwrap_or(ProtocolError::Disconnected))
}

fn send_on_connection(
    payload: &OutboundPayload,
    mut connection: quickshare_connections::Connection,
    share_id: u64,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
    manager: Option<&NetworkManager>,
) -> Result<u64, ProtocolError> {
    if cancellation.is_cancelled(share_id) {
        return Err(ProtocolError::Cancelled);
    }
    let _wifi = initiate_bandwidth_upgrade(&mut connection, manager)?;
    let medium = medium_name(connection.medium());
    let mut session = sharing_session(connection);
    events
        .send(NetworkEvent::OutboundPairing {
            share_id,
            verification_code: String::from(session.verification_code()),
        })
        .map_err(|error| {
            ProtocolError::Io(io::Error::new(io::ErrorKind::BrokenPipe, error))
        })?;
    let pairing = session.exchange_account_free_pairing();
    trace_paired_key_exchange(&pairing, "outbound", medium, Some(share_id));
    let _pairing =
        pairing.map_err(quickshare_sharing::PairingError::into_source)?;
    let on_accepted = || {
        let _result = events.send(NetworkEvent::OutboundAccepted { share_id });
    };
    let on_progress = |transferred_bytes| {
        emit_progress(events, medium, share_id, transferred_bytes);
    };
    let is_cancelled = || cancellation.is_cancelled(share_id);
    match payload {
        OutboundPayload::File(source) => send_file(
            &mut session,
            source,
            on_accepted,
            on_progress,
            is_cancelled,
        )
        .inspect_err(|error| trace_payload_failure(share_id, error)),
        OutboundPayload::Text(value) => {
            session
                .send_outgoing_text(
                    value,
                    on_accepted,
                    on_progress,
                    is_cancelled,
                )
                .inspect_err(|error| trace_payload_failure(share_id, error))?;
            Ok(u64::try_from(value.len()).unwrap_or(0))
        }
        OutboundPayload::Url(value) => {
            session
                .send_outgoing_url(
                    value,
                    on_accepted,
                    on_progress,
                    is_cancelled,
                )
                .inspect_err(|error| trace_payload_failure(share_id, error))?;
            Ok(u64::try_from(value.len()).unwrap_or(0))
        }
    }
}

fn send_file<Accepted, Progress, Cancelled>(
    session: &mut SharingSession,
    source: &OutboundSource,
    on_accepted: Accepted,
    on_progress: Progress,
    is_cancelled: Cancelled,
) -> Result<u64, ProtocolError>
where
    Accepted: FnOnce(),
    Progress: FnMut(u64),
    Cancelled: FnMut() -> bool,
{
    let name = source.name().to_str().ok_or_else(|| {
        ProtocolError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file name is not UTF-8",
        ))
    })?;
    let mut reader = source
        .reader()
        .map_err(|error| ProtocolError::Io(io::Error::other(error)))?;
    let size = source.len();
    session.send_outgoing_file(
        name,
        size,
        &mut reader,
        on_accepted,
        on_progress,
        is_cancelled,
    )?;
    Ok(size)
}

fn trace_payload_failure(share_id: u64, error: &ProtocolError) {
    if matches!(error, ProtocolError::Cancelled | ProtocolError::Rejected) {
        return;
    }
    tracing::warn!(
        share_id,
        stage = "payload_transfer",
        error_class = protocol_reason(error),
        "share failed"
    );
}
