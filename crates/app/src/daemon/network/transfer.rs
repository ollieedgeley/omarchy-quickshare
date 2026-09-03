//! Outbound LAN session and transfer event mapping.

use core::net::SocketAddrV4;
use std::{io, sync::mpsc::Sender};

use quickshare_network::lan::connect;
use quickshare_sharing::{ProtocolError, SharingSession};
use quickshare_storage::OutboundSource;

use super::{ENDPOINT_ID, ENDPOINT_NAME, NetworkEvent, TransferCancellation};
use crate::daemon::outbound::OutboundTransfer;

/// Converts one worker transfer attempt into a terminal daemon event.
pub(super) fn outbound_event(
    share_id: u64,
    transfer: &OutboundTransfer,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
) -> NetworkEvent {
    let event = match send_file(
        transfer.source(),
        transfer.route(),
        share_id,
        events,
        cancellation,
    ) {
        Ok(bytes) => NetworkEvent::OutboundCompleted { bytes, share_id },
        Err(ProtocolError::Cancelled) => {
            NetworkEvent::OutboundCancelled { share_id }
        }
        Err(ProtocolError::Rejected) => {
            NetworkEvent::OutboundRejected { share_id }
        }
        Err(error) => NetworkEvent::OutboundFailed {
            reason: error.to_string(),
            share_id,
        },
    };
    cancellation.finish(share_id);
    event
}

/// Streams one complete file over an encrypted account-free session.
fn send_file(
    source: &OutboundSource,
    route: SocketAddrV4,
    share_id: u64,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
) -> Result<u64, ProtocolError> {
    if cancellation.is_cancelled(share_id) {
        return Err(ProtocolError::Cancelled);
    }
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
    let stream = connect(route)?;
    let mut session =
        SharingSession::connect(stream, ENDPOINT_ID, ENDPOINT_NAME)?;
    events
        .send(NetworkEvent::OutboundPairing {
            share_id,
            verification_code: String::from(session.verification_code()),
        })
        .map_err(|error| {
            ProtocolError::Io(io::Error::new(io::ErrorKind::BrokenPipe, error))
        })?;
    let _pairing = session.exchange_account_free_pairing()?;
    session.send_outgoing_file(
        name,
        size,
        &mut reader,
        || {
            let _result =
                events.send(NetworkEvent::OutboundAccepted { share_id });
        },
        || cancellation.is_cancelled(share_id),
    )?;
    Ok(size)
}
