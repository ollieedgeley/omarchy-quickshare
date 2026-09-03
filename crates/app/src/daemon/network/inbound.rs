//! Incoming LAN advertisement, consent, and file persistence.

use alloc::collections::BTreeMap;
use std::io;
use std::net::TcpStream;
use std::sync::mpsc::{Receiver, Sender};

use quickshare_network::{
    Advertisement, DnsSd,
    lan::{Listener, PublishedLanListener},
    local_ipv4_addresses,
};
use quickshare_sharing::{EndpointInfo, IncomingFile, MdnsInstance};
use quickshare_sharing::{IncomingOffer, SharingSession};
use quickshare_storage::ReceiveTarget;

use super::{ENDPOINT_ID, ENDPOINT_NAME, NetworkCommand, NetworkEvent};

/// Hostname published with the first daemon identity.
const HOSTNAME: &str = "omarchy-quickshare.local.";

/// Creates and advertises the platform-owned incoming TCP listener.
pub(super) fn open_listener(
    dns_sd: &DnsSd,
) -> io::Result<PublishedLanListener> {
    let listener = Listener::bind_any()?;
    let advertisement = advertisement(listener.port())?;
    listener.publish(dns_sd, &advertisement)
}

/// Encrypts one sender session, waits for consent, and saves its file.
pub(super) fn receive_file(
    stream: TcpStream,
    commands: &Receiver<NetworkCommand>,
    events: &Sender<NetworkEvent>,
) -> NetworkEvent {
    match receive_file_result(stream, commands, events) {
        Ok((bytes, share_id)) => {
            NetworkEvent::InboundCompleted { bytes, share_id }
        }
        Err((error, share_id)) => NetworkEvent::InboundFailed {
            reason: error.to_string(),
            share_id,
        },
    }
}

/// Runs the fallible inbound protocol while retaining the accepted share ID.
fn receive_file_result(
    stream: TcpStream,
    commands: &Receiver<NetworkCommand>,
    events: &Sender<NetworkEvent>,
) -> Result<(u64, u64), (io::Error, Option<u64>)> {
    let mut session =
        SharingSession::accept(stream, ENDPOINT_ID, ENDPOINT_NAME)
            .map_err(|error| (io::Error::other(error), None))?;
    let _pairing = session
        .exchange_account_free_pairing()
        .map_err(|error| (io::Error::other(error), None))?;
    let offer = session
        .receive_incoming_offer()
        .map_err(|error| (io::Error::other(error), None))?;
    announce_offer(&offer, events).map_err(|error| (error, None))?;
    let share_id = wait_for_consent(commands).map_err(|error| (error, None))?;
    session
        .accept_incoming_offer()
        .map_err(|error| (io::Error::other(error), Some(share_id)))?;
    let file = session
        .receive_incoming_file(&offer)
        .map_err(|error| (io::Error::other(error), Some(share_id)))?;
    save_file(&file).map_err(|error| (error, Some(share_id)))?;
    let bytes = u64::try_from(file.bytes().len())
        .map_err(|error| (io::Error::other(error), Some(share_id)))?;
    Ok((bytes, share_id))
}

/// Publishes the validated offer to the daemon state owner.
fn announce_offer(
    offer: &IncomingOffer,
    events: &Sender<NetworkEvent>,
) -> io::Result<()> {
    let size_bytes = u64::try_from(offer.size_bytes())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    events
        .send(NetworkEvent::InboundOffered {
            name: offer.name().to_owned(),
            size_bytes,
        })
        .map_err(|error| io::Error::new(io::ErrorKind::BrokenPipe, error))
}

/// Waits for the local-control owner to approve the pending sender.
fn wait_for_consent(commands: &Receiver<NetworkCommand>) -> io::Result<u64> {
    loop {
        match commands.recv() {
            Ok(NetworkCommand::AcceptInbound { share_id }) => {
                return Ok(share_id);
            }
            Ok(NetworkCommand::CloseVisibility) => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "visibility closed before local consent",
                ));
            }
            Ok(_) => {}
            Err(error) => {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, error));
            }
        }
    }
}

/// Persists a validated file without replacing an existing user file.
fn save_file(file: &IncomingFile) -> io::Result<()> {
    let target = ReceiveTarget::downloads().map_err(io::Error::other)?;
    let mut staged = target.stage(file.name()).map_err(io::Error::other)?;
    staged.write_all(file.bytes()).map_err(io::Error::other)?;
    let _destination = staged.commit().map_err(io::Error::other)?;
    Ok(())
}

/// Builds the Google-compatible DNS-SD record for the bound listener.
fn advertisement(port: u16) -> io::Result<Advertisement> {
    let endpoint = EndpointInfo::new(
        0,
        5,
        [0; 2],
        [0; 14],
        Some(ENDPOINT_NAME),
        None,
        Vec::new(),
    )
    .map_err(io::Error::other)?;
    let properties = BTreeMap::from([(String::from("n"), endpoint.property())]);
    Ok(Advertisement {
        addresses: local_ipv4_addresses().map_err(io::Error::other)?,
        hostname: String::from(HOSTNAME),
        instance: MdnsInstance::new(*b"OQSR").label(),
        port,
        properties,
        service_type: MdnsInstance::service_type().to_owned(),
    })
}
