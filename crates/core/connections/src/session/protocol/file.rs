use super::{
    payload_chunk_received, payload_debug_progress, payload_debug_progress_at,
    transfer::payload_rejected,
};
use crate::{Connection, Error, Event};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Connection methods remain grouped by protocol operation"
)]
impl Connection {
    pub(super) fn file_payload(
        &mut self,
        id: i64,
        size: i64,
        name: Option<String>,
        offset: i64,
        bytes: Vec<u8>,
        last: bool,
    ) -> Result<Event, Error> {
        let first_chunk = self.begin_file(id, size, offset)?;
        let byte_count = bytes.len();
        self.advance_file(id, size, offset, byte_count, last)?;
        payload_chunk_received(offset, byte_count);
        if first_chunk {
            payload_debug_progress("receive", "started", "file", size);
        }
        let event = Event::FileChunk {
            id,
            offset,
            bytes,
            is_last: last,
        };
        if last {
            let _ = self.incoming_files.remove(&id);
            payload_debug_progress_at(
                "receive",
                "completed",
                "file",
                offset,
                byte_count,
            );
        }
        if first_chunk {
            self.pending_events.push_back(event);
            return Ok(Event::FileHeader {
                id,
                total_size: size,
                name,
            });
        }
        Ok(event)
    }
    fn begin_file(
        &mut self,
        id: i64,
        size: i64,
        offset: i64,
    ) -> Result<bool, Error> {
        let first_chunk = !self.incoming_files.contains_key(&id);
        if !first_chunk {
            return Ok(false);
        }
        if offset != 0 {
            payload_rejected("receive", "invalid_initial_offset");
            return Err(Error::InvalidPayload);
        }
        let _ = self.incoming_files.insert(
            id,
            super::super::IncomingFile {
                next_offset: 0,
                size,
            },
        );
        Ok(true)
    }

    fn advance_file(
        &mut self,
        id: i64,
        size: i64,
        offset: i64,
        byte_count: usize,
        last: bool,
    ) -> Result<(), Error> {
        let chunk_size = i64::try_from(byte_count).map_err(|_| {
            payload_rejected("receive", "chunk_too_large");
            Error::InvalidPayload
        })?;
        let end = offset.checked_add(chunk_size).ok_or_else(|| {
            payload_rejected("receive", "offset_overflow");
            Error::InvalidPayload
        })?;
        let file = self
            .incoming_files
            .get_mut(&id)
            .ok_or(Error::InvalidPayload)?;
        if file.size != size
            || file.next_offset != offset
            || end > size
            || (last && end != size)
        {
            payload_rejected("receive", "invalid_file_chunk");
            return Err(Error::InvalidPayload);
        }
        file.next_offset = end;
        Ok(())
    }
}
