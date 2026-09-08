//! Durable identity is committed before a submission can start network work.

use std::io;

use quickshare_control::request::{Envelope, Request};
use quickshare_control::response::{
    Envelope as Reply, ErrorCode, OperationRecord,
};
use quickshare_sharing::{Attachment, Direction, Phase};

use super::{Daemon, journal::Journal};

pub(super) const fn terminal(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::Completed | Phase::Cancelled | Phase::Rejected | Phase::Failed
    )
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Operation admission owns durable acceptance"
)]
impl Daemon {
    pub(super) fn attach_journal(
        &mut self,
        journal: Journal,
    ) -> io::Result<()> {
        if !self.sharing.restore_next_share_id(journal.next_id()) {
            return Err(io::Error::other("invalid durable operation counter"));
        }
        self.journal = journal;
        Ok(())
    }

    pub(super) fn persist_operations(&mut self) -> io::Result<()> {
        let Some(share) = self.sharing.snapshot().active_share() else {
            return Ok(());
        };
        let id_string = share.id().get().to_string();
        let previous = self
            .journal
            .records()
            .iter()
            .find(|record| record.id_string == id_string);
        let record = OperationRecord {
            direction: share.direction(),
            dismissed: previous.is_some_and(|record| record.dismissed),
            id_string,
            phase: share.phase(),
            recovery_guidance: share.recovery_guidance().map(str::to_owned),
            request_id: previous.and_then(|record| record.request_id.clone()),
            terminal_reason: share.terminal_reason().map(str::to_owned),
            total_bytes: share.total_bytes(),
            transferred_bytes: share.transferred_bytes(),
        };
        let finished_id = terminal(record.phase).then_some(share.id().get());
        self.journal.update(record, self.sharing.next_share_id())?;
        if let Some(share_id) = finished_id {
            self.outbound.finish(share_id);
        }
        Ok(())
    }

    pub(super) fn response_for(
        &mut self,
        envelope: &Envelope,
    ) -> io::Result<Reply> {
        if self.apply_timeouts().is_err() || self.persist_operations().is_err()
        {
            return Ok(Reply::error(ErrorCode::PersistenceFailed));
        }
        if let Request::Operation { request_id } = envelope.request() {
            return Ok(self.journal.request(request_id).map_or_else(
                || Reply::error(ErrorCode::OutcomeUnknown),
                |operation| Reply::operation(operation.clone()),
            ));
        }
        if let Some(request_id) = envelope.request_id() {
            return self.submit_operation(envelope.request(), request_id);
        }
        let response = self.apply_request(envelope.request());
        if self.persist_operations().is_err() {
            return Ok(Reply::error(ErrorCode::PersistenceFailed));
        }
        response
    }

    fn submission_allowed(
        &self,
        request_id: &str,
        peer_id: Option<&str>,
    ) -> Result<(), ErrorCode> {
        if self.journal.request(request_id).is_some() {
            return Err(ErrorCode::Conflict);
        }
        if self
            .sharing
            .snapshot()
            .active_share()
            .is_some_and(|share| !terminal(share.phase()))
        {
            return Err(ErrorCode::Busy);
        }
        if self.sharing.next_share_id() == u64::MAX {
            return Err(ErrorCode::PersistenceFailed);
        }
        if peer_id.is_some_and(|id| {
            !self
                .sharing
                .snapshot()
                .peers()
                .iter()
                .any(|peer| peer.id() == id)
        }) {
            return Err(ErrorCode::InvalidRequest);
        }
        Ok(())
    }

    fn submit_operation(
        &mut self,
        request: &Request,
        request_id: &str,
    ) -> io::Result<Reply> {
        let peer_id = match request {
            Request::SubmitFile { peer_id, .. }
            | Request::SubmitText { peer_id, .. }
            | Request::SubmitUrl { peer_id, .. } => peer_id.as_deref(),
            _ => return Ok(Reply::error(ErrorCode::InvalidRequest)),
        };
        if let Err(code) = self.submission_allowed(request_id, peer_id) {
            return Ok(Reply::error(code));
        }
        let (attachment, source, archive) = match request {
            Request::SubmitFile { path, .. } => {
                let (attachment, source, archive) = Self::prepare_file(path)?;
                (attachment, Some(source), archive)
            }
            Request::SubmitText { text, .. } => {
                (Attachment::text(text), None, None)
            }
            Request::SubmitUrl { url, .. } => {
                (Attachment::url(url), None, None)
            }
            _ => return Ok(Reply::error(ErrorCode::InvalidRequest)),
        };
        let total_bytes = source.as_ref().map_or_else(
            || {
                let length = match request {
                    Request::SubmitText { text, .. } => text.len(),
                    Request::SubmitUrl { url, .. } => url.len(),
                    _ => 0,
                };
                u64::try_from(length).unwrap_or(u64::MAX)
            },
            quickshare_storage::OutboundSource::len,
        );
        let share_id = self.sharing.next_share_id();
        let operation = OperationRecord {
            direction: Direction::Outbound,
            dismissed: false,
            id_string: share_id.to_string(),
            phase: Phase::WaitingForPeer,
            recovery_guidance: None,
            request_id: Some(request_id.to_owned()),
            terminal_reason: None,
            total_bytes,
            transferred_bytes: 0,
        };
        if self.journal.update(operation, share_id + 1).is_err() {
            if let Some(path) = archive {
                let _removed = std::fs::remove_file(path);
            }
            return Ok(Reply::error(ErrorCode::PersistenceFailed));
        }
        let accepted_id =
            self.queue_attachment_for(attachment, peer_id.is_some());
        if let Some(source) = source {
            self.outbound.remember_file(accepted_id, source);
        }
        if let Some(path) = archive {
            self.outbound.remember_archive(accepted_id, path);
        }
        match request {
            Request::SubmitText { text, .. } => {
                self.outbound.remember_text(accepted_id, text.clone());
            }
            Request::SubmitUrl { url, .. } => {
                self.outbound.remember_url(accepted_id, url.clone());
            }
            _ => {}
        }
        if self.start_accepted_operation(accepted_id, peer_id).is_err() {
            return Ok(Reply::error(ErrorCode::PersistenceFailed));
        }
        Ok(self.journal.request(request_id).map_or_else(
            || Reply::error(ErrorCode::PersistenceFailed),
            |record| Reply::queued(record.clone()),
        ))
    }

    fn start_accepted_operation(
        &mut self,
        share_id: u64,
        peer_id: Option<&str>,
    ) -> io::Result<()> {
        self.sharing.start_discovery();
        let discovery = self
            .network
            .as_ref()
            .map_or(Ok(()), |network| network.discover());
        if discovery.is_err() {
            let _failed = self.sharing.fail(share_id);
            let _observed = self.sharing.record_observation(
                share_id,
                None,
                None,
                Some("discovery_failed"),
                Some(
                    "No transfer was started. \
                     Review the operation before submitting new work.",
                ),
            );
        } else if let Some(peer_id) = peer_id {
            let _selected = self.select_peer(share_id, peer_id);
        } else {
            let _started = self.start_pinned_outbound(share_id);
        }
        self.persist_operations()
    }
}
