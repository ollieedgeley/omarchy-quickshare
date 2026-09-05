use alloc::collections::BTreeMap;
use core::{net::Ipv4Addr, time::Duration};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Instant;
use std::{env, io};

use quickshare_connections::Medium;
use quickshare_network::{
    Advertisement, DnsSd, host_label,
    lan::{Listener, PublishedLanListener},
    local_ipv4_addresses,
};
use quickshare_sharing::{
    EndpointInfo, IncomingOffer, MdnsInstance, OfferKind, ProtocolError,
    SharingSession,
};
use quickshare_storage::{ReceiveTarget, StagedFile};

use super::{
    NetworkCommand, NetworkEvent, TransferCancellation, emit_progress,
};
use crate::daemon::media::{
    ENDPOINT_ID_BYTES, accept_connection, accept_negotiated_upgrade,
    endpoint_name, medium_name, sharing_session,
};
use crate::daemon::observations::{
    connection_span, protocol_io_kind, protocol_reason, record_share_id,
    storage_reason, trace_paired_key_exchange, trace_protocol, trace_storage,
};

const LAN_PORT: u16 = 53_318;
const TEST_LAN_PORT: &str = "OMARCHY_QUICKSHARE_TEST_LAN_PORT";
const CONSENT_POLL: Duration = Duration::from_millis(50);

enum Consent {
    Accepted(u64),
    Rejected(u64),
    TimedOut,
}

pub(super) fn open_listener(
    dns_sd: &DnsSd,
) -> io::Result<PublishedLanListener> {
    let result = (|| {
        let port = env::var(TEST_LAN_PORT)
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(LAN_PORT);
        let listener = Listener::bind(port)?;
        let advertisement = advertisement(listener.port())?;
        listener.publish(dns_sd, &advertisement)
    })();
    let (outcome, reason, io_error_kind) = match &result {
        Ok(_) => ("completed", None, None),
        Err(error) => (
            "failed",
            Some("io"),
            Some(crate::daemon::observations::io_error_kind(error)),
        ),
    };
    trace_protocol(
        "visibility",
        "open_listener",
        outcome,
        reason,
        io_error_kind,
    );
    result
}

pub(super) fn receive_share<Stream>(
    stream: Stream,
    medium: Medium,
    commands: &Receiver<NetworkCommand>,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
    receive_directory: &Path,
    consent_deadline: Duration,
    manager: Option<&quickshare_network::NetworkManager>,
    on_other: &mut dyn FnMut(NetworkCommand) -> bool,
) -> NetworkEvent
where
    Stream: quickshare_connections::ConnectionIo + 'static,
{
    let connection_span = connection_span("inbound", medium_name(medium), None);
    let _connection_guard = connection_span.enter();
    trace_protocol("connection", "receive", "started", None, None);
    let event = match (|| -> Result<NetworkEvent, (String, Option<u64>)> {
        let mut connection =
            accept_connection(stream, medium).map_err(|error| {
                trace_protocol(
                    "connection",
                    "accept",
                    "failed",
                    Some(protocol_reason(&error)),
                    protocol_io_kind(&error),
                );
                (String::from(protocol_reason(&error)), None)
            })?;
        let _wifi = accept_negotiated_upgrade(&mut connection, manager)
            .map_err(|error| {
                trace_protocol(
                    "upgrade",
                    "accept",
                    "failed",
                    Some(protocol_reason(&error)),
                    protocol_io_kind(&error),
                );
                (String::from(protocol_reason(&error)), None)
            })?;
        let pairing_medium = medium_name(connection.medium());
        let mut session = sharing_session(connection);
        let pairing = session.exchange_account_free_pairing();
        trace_paired_key_exchange(&pairing, "inbound", pairing_medium, None);
        let _pairing = pairing.map_err(|error| {
            (String::from(protocol_reason(error.source_error())), None)
        })?;
        let offer = session.receive_incoming_offer().map_err(|error| {
            trace_protocol(
                "consent",
                "receive_offer",
                "failed",
                Some(protocol_reason(&error)),
                protocol_io_kind(&error),
            );
            (String::from(protocol_reason(&error)), None)
        })?;
        announce_offer(&offer, session.verification_code(), events).map_err(
            |error| {
                trace_protocol(
                    "consent",
                    "announce_offer",
                    "failed",
                    Some("disconnected"),
                    Some(crate::daemon::observations::io_error_kind(&error)),
                );
                (String::from("disconnected"), None)
            },
        )?;
        let consent = wait_for_consent(commands, consent_deadline, on_other)?;
        let share_id = match consent {
            Consent::Accepted(share_id) => {
                record_share_id(&connection_span, share_id);
                trace_protocol("consent", "accept", "completed", None, None);
                share_id
            }
            Consent::Rejected(share_id) => {
                record_share_id(&connection_span, share_id);
                trace_protocol("consent", "reject", "completed", None, None);
                session.reject_incoming_offer().map_err(|error| {
                    (String::from(protocol_reason(&error)), Some(share_id))
                })?;
                cancellation.finish(share_id);
                return Ok(NetworkEvent::InboundRejected { share_id });
            }
            Consent::TimedOut => {
                trace_protocol(
                    "consent",
                    "deadline",
                    "timed_out",
                    Some("timed_out"),
                    None,
                );
                session.timeout_incoming_offer().map_err(|error| {
                    (String::from(protocol_reason(&error)), None)
                })?;
                return Err((String::from("timed_out"), None));
            }
        };
        let bytes = u64::try_from(offer.size_bytes())
            .map_err(|_| (String::from("invalid_payload"), Some(share_id)))?;
        let mut staged = if offer.kind().persists_as_file() {
            let target =
                ReceiveTarget::open(receive_directory).map_err(|error| {
                    payload_storage_failure(
                        share_id,
                        "open_receive_target",
                        &error,
                    )
                })?;
            trace_storage("open_receive_target", "completed", None);
            target.preflight(bytes).map_err(|error| {
                payload_storage_failure(share_id, "preflight", &error)
            })?;
            let mut files = Vec::with_capacity(offer.file_count());
            for index in 0..offer.file_count() {
                let part = offer
                    .file(index)
                    .ok_or((String::from("invalid_payload"), Some(share_id)))?;
                let size = u64::try_from(part.size_bytes()).map_err(|_| {
                    (String::from("invalid_payload"), Some(share_id))
                })?;
                files.push(target.stage(part.name(), size).map_err(
                    |error| payload_storage_failure(share_id, "stage", &error),
                )?);
            }
            trace_storage("stage", "completed", None);
            files
        } else {
            Vec::new()
        };
        session.accept_incoming_offer().map_err(|error| {
            (String::from(protocol_reason(&error)), Some(share_id))
        })?;
        let transfer = receive_payload(
            &mut session,
            &offer,
            &mut staged,
            share_id,
            medium_name(medium),
            events,
            cancellation,
        );
        cancellation.finish(share_id);
        match transfer {
            Ok(event) => {
                for file in staged {
                    drop(file.commit().map_err(|error| {
                        payload_storage_failure(share_id, "commit", &error)
                    })?);
                }
                trace_storage("commit", "completed", None);
                Ok(event)
            }
            Err(ProtocolError::Cancelled) => {
                Ok(NetworkEvent::InboundCancelled { share_id })
            }
            Err(error) => {
                trace_protocol(
                    "payload_transfer",
                    "receive",
                    "failed",
                    Some(protocol_reason(&error)),
                    protocol_io_kind(&error),
                );
                Err((String::from(protocol_reason(&error)), Some(share_id)))
            }
        }
    })() {
        Ok(event) => event,
        Err((reason, share_id)) => {
            NetworkEvent::InboundFailed { reason, share_id }
        }
    };
    let (outcome, reason) = match &event {
        NetworkEvent::InboundCompleted { .. } => ("completed", None),
        NetworkEvent::InboundRejected { .. } => ("rejected", None),
        NetworkEvent::InboundCancelled { .. } => ("cancelled", None),
        NetworkEvent::InboundFailed { reason, .. } => {
            ("failed", Some(reason.as_str()))
        }
        _ => ("failed", Some("unexpected_event")),
    };
    trace_protocol("connection", "receive", outcome, reason, None);
    event
}

fn payload_storage_failure(
    share_id: u64,
    operation: &'static str,
    error: &quickshare_storage::Error,
) -> (String, Option<u64>) {
    trace_storage(operation, "failed", Some(error));
    (String::from(storage_reason(error)), Some(share_id))
}

fn receive_payload(
    session: &mut SharingSession,
    offer: &IncomingOffer,
    staged: &mut [StagedFile],
    share_id: u64,
    medium: &'static str,
    events: &Sender<NetworkEvent>,
    cancellation: &TransferCancellation,
) -> Result<NetworkEvent, ProtocolError> {
    let (bytes, value) = if offer.kind().persists_as_file() {
        if staged.len() != offer.file_count() {
            return Err(ProtocolError::InvalidPayload);
        }
        let mut received = 0_u64;
        for (index, writer) in staged.iter_mut().enumerate() {
            let part =
                offer.file(index).ok_or(ProtocolError::InvalidPayload)?;
            let prior = received;
            session.receive_incoming_file(
                &part,
                writer,
                |transferred_bytes| {
                    emit_progress(
                        events,
                        medium,
                        share_id,
                        prior.saturating_add(transferred_bytes),
                    );
                },
                || cancellation.is_cancelled(share_id),
            )?;
            received = prior.saturating_add(
                u64::try_from(part.size_bytes())
                    .map_err(|_| ProtocolError::InvalidPayload)?,
            );
        }
        (received, None)
    } else {
        let on_progress = |transferred_bytes| {
            emit_progress(events, medium, share_id, transferred_bytes);
        };
        let is_cancelled = || cancellation.is_cancelled(share_id);
        match offer.kind() {
            OfferKind::Text => {
                let value = session.receive_incoming_text(
                    offer,
                    on_progress,
                    is_cancelled,
                )?;
                (u64::try_from(value.len()).unwrap_or(0), Some(value))
            }
            OfferKind::Url => {
                let value = session.receive_incoming_url(
                    offer,
                    on_progress,
                    is_cancelled,
                )?;
                (u64::try_from(value.len()).unwrap_or(0), Some(value))
            }
            OfferKind::AndroidApp | OfferKind::File => {
                return Err(ProtocolError::InvalidPayload);
            }
        }
    };
    Ok(NetworkEvent::InboundCompleted {
        bytes,
        kind: offer.kind(),
        share_id,
        value,
    })
}

fn announce_offer(
    offer: &IncomingOffer,
    verification_code: &str,
    events: &Sender<NetworkEvent>,
) -> io::Result<()> {
    let size_bytes = u64::try_from(offer.size_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    events
        .send(NetworkEvent::InboundOffered {
            kind: offer.kind(),
            name: offer.name().to_owned(),
            size_bytes,
            verification_code: String::from(verification_code),
        })
        .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))
}

fn wait_for_consent(
    commands: &Receiver<NetworkCommand>,
    deadline: Duration,
    on_other: &mut dyn FnMut(NetworkCommand) -> bool,
) -> Result<Consent, (String, Option<u64>)> {
    let started = Instant::now();
    loop {
        if started.elapsed() >= deadline {
            return Ok(Consent::TimedOut);
        }
        match commands.recv_timeout(CONSENT_POLL) {
            Ok(NetworkCommand::AcceptInbound { share_id }) => {
                return Ok(Consent::Accepted(share_id));
            }
            Ok(NetworkCommand::RejectInbound { share_id }) => {
                return Ok(Consent::Rejected(share_id));
            }
            Ok(command) => {
                let close = matches!(command, NetworkCommand::CloseVisibility);
                if !on_other(command) {
                    return Err((String::from("disconnected"), None));
                }
                if close {
                    return Err((String::from("cancelled"), None));
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                return Err((String::from("disconnected"), None));
            }
        }
    }
}

fn advertisement(port: u16) -> io::Result<Advertisement> {
    advertisement_for(
        port,
        endpoint_name(),
        local_ipv4_addresses().map_err(io::Error::other)?,
    )
}

fn advertisement_for(
    port: u16,
    name: &str,
    addresses: Vec<Ipv4Addr>,
) -> io::Result<Advertisement> {
    let endpoint =
        EndpointInfo::new(0, 3, [0; 2], [0; 14], Some(name), None, Vec::new())
            .map_err(io::Error::other)?;
    let properties = BTreeMap::from([(String::from("n"), endpoint.property())]);
    Ok(Advertisement {
        addresses,
        hostname: format!("{}.local.", host_label(name)),
        instance: MdnsInstance::new(ENDPOINT_ID_BYTES).label(),
        port,
        properties,
        service_type: MdnsInstance::service_type().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Consent, LAN_PORT, NetworkCommand, advertisement_for, wait_for_consent,
    };
    use core::net::Ipv4Addr;
    use quickshare_sharing::EndpointInfo;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn stop_discovery_during_consent_is_dispatched_before_accept() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(NetworkCommand::StopDiscovery)
            .expect("queue stop");
        sender
            .send(NetworkCommand::AcceptInbound { share_id: 4 })
            .expect("queue accept");
        let mut dispatched = 0_u8;
        let consent = wait_for_consent(
            &receiver,
            Duration::from_secs(1),
            &mut |command| {
                assert!(matches!(command, NetworkCommand::StopDiscovery));
                dispatched = dispatched.saturating_add(1);
                true
            },
        )
        .expect("accepted");
        assert!(matches!(consent, Consent::Accepted(4)));
        assert_eq!(dispatched, 1);
    }

    #[test]
    fn close_visibility_during_consent_is_dispatched_and_cancels() {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(NetworkCommand::CloseVisibility)
            .expect("queue close");
        let mut dispatched = false;
        let result = wait_for_consent(
            &receiver,
            Duration::from_secs(1),
            &mut |command| {
                dispatched = matches!(command, NetworkCommand::CloseVisibility);
                true
            },
        );
        assert!(dispatched);
        assert!(matches!(result, Err((reason, None)) if reason == "cancelled"));
    }

    #[test]
    fn consent_timeout_is_terminal() {
        let (_sender, receiver) = mpsc::channel::<NetworkCommand>();
        let consent =
            wait_for_consent(&receiver, Duration::from_millis(0), &mut |_| {
                true
            })
            .expect("timed out");
        assert!(matches!(consent, Consent::TimedOut));
    }

    #[test]
    fn advertisement_uses_laptop_hostname_and_fixed_port() {
        let record = advertisement_for(
            LAN_PORT,
            "Ollie's MacBook",
            vec![Ipv4Addr::LOCALHOST],
        )
        .expect("advertisement");
        let endpoint = EndpointInfo::decode_property(
            record.properties.get("n").expect("endpoint property"),
        )
        .expect("endpoint info");
        assert_eq!((endpoint.encode()[0] >> 1) & 7, 3);
        assert_eq!(endpoint.device_name(), Some("Ollie's MacBook"));
        assert_eq!(record.hostname, "ollie-s-macbook.local.");
        assert_eq!(record.port, LAN_PORT);
    }
}
