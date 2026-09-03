//! Private routing and source paths for outbound production transfers.

#![expect(
    clippy::pub_with_shorthand,
    reason = "rustfmt scoped visibility conflicts with the restriction lint"
)]

use alloc::sync::Arc;
use core::net::SocketAddrV4;
use std::collections::HashMap;

use quickshare_storage::OutboundSource;

/// Everything the network worker needs to start one selected file transfer.
#[derive(Clone, Debug)]
pub(super) struct OutboundTransfer {
    /// Address advertised by the selected peer.
    route: SocketAddrV4,
    /// Open source captured when the local client queued the share.
    source: Arc<OutboundSource>,
}

/// Daemon-private facts that must not leak into the control protocol.
#[derive(Debug, Default)]
pub(super) struct OutboundState {
    /// Latest LAN route keyed by advertised peer identifier.
    routes: HashMap<String, SocketAddrV4>,
    /// Open sources keyed by stable local share identifier.
    sources: HashMap<u64, Arc<OutboundSource>>,
}

impl OutboundState {
    /// Forgets private state after a transfer reaches a terminal phase.
    pub(super) fn finish(&mut self, share_id: u64) {
        drop(self.sources.remove(&share_id));
    }

    /// Records the open source for a newly queued file.
    pub(super) fn remember_file(
        &mut self,
        share_id: u64,
        source: OutboundSource,
    ) {
        drop(self.sources.insert(share_id, Arc::new(source)));
    }

    /// Records or refreshes a discovered peer's private route.
    pub(super) fn remember_peer(&mut self, peer_id: &str, route: SocketAddrV4) {
        let _previous_route = self.routes.insert(peer_id.to_owned(), route);
    }

    /// Resolves a selected share and peer into one worker-owned transfer.
    pub(super) fn transfer(
        &self,
        share_id: u64,
        peer_id: &str,
    ) -> Option<OutboundTransfer> {
        Some(OutboundTransfer {
            route: *self.routes.get(peer_id)?,
            source: Arc::clone(self.sources.get(&share_id)?),
        })
    }
}

impl OutboundTransfer {
    /// Returns the selected peer's private TCP route.
    pub(super) const fn route(&self) -> SocketAddrV4 {
        self.route
    }

    /// Returns the open source captured for this transfer.
    pub(super) fn source(&self) -> &OutboundSource {
        &self.source
    }
}
