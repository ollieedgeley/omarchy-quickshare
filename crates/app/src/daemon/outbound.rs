//! Private routing and payloads for outbound production transfers.

#![expect(
    clippy::pub_with_shorthand,
    reason = "rustfmt scoped visibility conflicts with the restriction lint"
)]

use alloc::sync::Arc;
use std::collections::HashMap;

use quickshare_storage::OutboundSource;

use super::media::{PeerRoute, attempt_order};

/// Content the network worker sends after a peer is selected.
#[derive(Clone, Debug)]
pub(super) enum OutboundPayload {
    /// Open file captured when the local client queued the share.
    File(Arc<OutboundSource>),
    /// Exact outbound text.
    Text(String),
    /// Exact outbound URL.
    Url(String),
}

/// Everything the network worker needs to start one selected transfer.
#[derive(Clone, Debug)]
pub(super) struct OutboundTransfer {
    /// Payload queued for this share.
    payload: OutboundPayload,
    /// Private candidate routes, LAN first when present.
    routes: Vec<PeerRoute>,
}

/// Daemon-private facts that must not leak into the control protocol.
#[derive(Debug, Default)]
pub(super) struct OutboundState {
    /// Temporary ZIP paths keyed by share identifier.
    archives: HashMap<u64, std::path::PathBuf>,
    /// Queued payloads keyed by stable local share identifier.
    payloads: HashMap<u64, OutboundPayload>,
    /// Discovered private routes keyed by advertised peer identifier.
    routes: HashMap<String, Vec<PeerRoute>>,
}

impl OutboundState {
    pub(super) fn finish(&mut self, share_id: u64) {
        drop(self.payloads.remove(&share_id));
        if let Some(archive) = self.archives.remove(&share_id) {
            crate::archive::remove_archive(&archive);
            crate::daemon::observations::trace_storage(
                "cleanup_archive",
                "attempted",
                None,
            );
        }
    }

    pub(super) fn forget_peer(&mut self, peer_id: &str) {
        drop(self.routes.remove(peer_id));
    }

    pub(super) fn remember_archive(
        &mut self,
        share_id: u64,
        archive: std::path::PathBuf,
    ) {
        drop(self.archives.insert(share_id, archive));
    }

    pub(super) fn remember_file(
        &mut self,
        share_id: u64,
        source: OutboundSource,
    ) {
        drop(
            self.payloads
                .insert(share_id, OutboundPayload::File(Arc::new(source))),
        );
    }

    pub(super) fn remember_peer(&mut self, peer_id: &str, route: PeerRoute) {
        let routes = self.routes.entry(peer_id.to_owned()).or_default();
        if !routes.contains(&route) {
            routes.push(route);
            routes.sort_by_key(|candidate| {
                attempt_order()
                    .iter()
                    .position(|&medium| medium == candidate.medium())
                    .unwrap_or(usize::MAX)
            });
        }
    }

    pub(super) fn remember_text(&mut self, share_id: u64, value: String) {
        drop(self.payloads.insert(share_id, OutboundPayload::Text(value)));
    }

    pub(super) fn remember_url(&mut self, share_id: u64, value: String) {
        drop(self.payloads.insert(share_id, OutboundPayload::Url(value)));
    }

    pub(super) fn transfer(
        &self,
        share_id: u64,
        peer_id: &str,
    ) -> Option<OutboundTransfer> {
        let routes = self
            .routes
            .get(peer_id)
            .filter(|routes| !routes.is_empty())?;
        Some(OutboundTransfer {
            payload: self.payloads.get(&share_id)?.clone(),
            routes: routes.clone(),
        })
    }
}

impl OutboundTransfer {
    pub(super) fn payload(&self) -> &OutboundPayload {
        &self.payload
    }

    pub(super) fn routes(&self) -> &[PeerRoute] {
        &self.routes
    }
}
