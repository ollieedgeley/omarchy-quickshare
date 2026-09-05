use super::{FILE_CHUNK_SIZE, FILE_PAYLOAD_ID, SharingSession};
use crate::protocol::{IncomingOffer, ProtocolError, frames, offer};
use core::time::Duration;
use quickshare_connections::Event;
use std::io::{Read, Write};

const OUTGOING_COMPLETION_TIMEOUT: Duration = Duration::from_secs(60);

impl SharingSession {
    /// Receives a file payload into a caller-owned destination.
    ///
    /// # Errors
    ///
    /// Returns an error when payload metadata, chunks, lengths, or writes fail.
    pub fn receive_incoming_file<Writer, Progress, Cancelled>(
        &mut self,
        offer: &IncomingOffer,
        writer: &mut Writer,
        mut on_progress: Progress,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Writer: Write,
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.stop_if_cancelled(&mut is_cancelled)?;
        let (id, size) = self.receive_file_header(offer)?;
        self.receive_file_chunks(
            id,
            size,
            writer,
            &mut on_progress,
            &mut is_cancelled,
        )
    }

    /// Introduces one file, reports peer consent, then streams its payload.
    ///
    /// # Errors
    ///
    /// Returns an error for unsafe names, invalid lengths, peer rejection, or
    /// transfer failure. The callback runs exactly once after peer acceptance
    /// and before the first payload frame.
    pub fn send_outgoing_file<Reader, Accepted, Progress, Cancelled>(
        &mut self,
        name: &str,
        size: u64,
        reader: &mut Reader,
        on_accepted: Accepted,
        mut on_progress: Progress,
        mut is_cancelled: Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Reader: Read,
        Accepted: FnOnce(),
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        if !offer::safe_name(name) {
            return Err(ProtocolError::InvalidOffer("unsafe file name"));
        }
        let wire_size =
            i64::try_from(size).map_err(|_| ProtocolError::InvalidPayload)?;
        if is_cancelled() {
            return Err(ProtocolError::Cancelled);
        }
        self.send_control_frame(&frames::introduction(name, wire_size))?;
        frames::consent_result(Self::decode_response(&self.receive_bytes()?)?)?;
        on_accepted();
        self.stop_if_cancelled(&mut is_cancelled)?;
        self.send_file_payload(
            name,
            reader,
            size,
            wire_size,
            &mut on_progress,
            &mut is_cancelled,
        )?;
        self.wait_for_outgoing_completion(FILE_PAYLOAD_ID, &mut is_cancelled)
    }

    fn receive_file_header(
        &mut self,
        offer: &IncomingOffer,
    ) -> Result<(i64, u64), ProtocolError> {
        loop {
            match self.next_transfer_event_for(offer.payload_id())? {
                Event::Bytes { bytes, .. } if frames::is_v1_control(&bytes) => {
                    if frames::is_cancel(&bytes)? {
                        return Err(ProtocolError::Cancelled);
                    }
                }
                Event::FileHeader {
                    id,
                    total_size,
                    name,
                } => {
                    if id != offer.payload_id()
                        || total_size != offer.size_bytes()
                        || name.as_deref() != Some(offer.name())
                    {
                        return Err(ProtocolError::InvalidPayload);
                    }
                    let size = u64::try_from(total_size)
                        .map_err(|_| ProtocolError::InvalidPayload)?;
                    return Ok((id, size));
                }
                Event::Disconnected => {
                    return Err(ProtocolError::Disconnected);
                }
                _ => return Err(ProtocolError::InvalidPayload),
            }
        }
    }

    fn receive_file_chunks<Writer, Progress, Cancelled>(
        &mut self,
        id: i64,
        size: u64,
        writer: &mut Writer,
        on_progress: &mut Progress,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Writer: Write,
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        let mut received = 0_u64;
        loop {
            self.stop_if_cancelled(is_cancelled)?;
            let event = self.next_transfer_event_for(id)?;
            if let Event::Bytes { bytes, .. } = &event
                && frames::is_v1_control(bytes)
            {
                if frames::is_cancel(bytes)? {
                    return Err(ProtocolError::Cancelled);
                }
                continue;
            }
            let Event::FileChunk {
                id: chunk_id,
                offset,
                bytes: chunk,
                is_last,
            } = event
            else {
                return if matches!(event, Event::Disconnected) {
                    Err(ProtocolError::Disconnected)
                } else {
                    Err(ProtocolError::InvalidPayload)
                };
            };
            let expected_offset = i64::try_from(received)
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let chunk_size = u64::try_from(chunk.len())
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let next_received = received
                .checked_add(chunk_size)
                .ok_or(ProtocolError::InvalidPayload)?;
            if chunk_id != id
                || offset != expected_offset
                || next_received > size
                || (chunk.is_empty() && !is_last)
            {
                return Err(ProtocolError::InvalidPayload);
            }
            writer.write_all(&chunk)?;
            received = next_received;
            on_progress(received);
            if is_last {
                return (received == size)
                    .then_some(())
                    .ok_or(ProtocolError::InvalidPayload);
            }
        }
    }

    fn send_file_payload<Reader, Progress, Cancelled>(
        &mut self,
        name: &str,
        reader: &mut Reader,
        size: u64,
        wire_size: i64,
        on_progress: &mut Progress,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Reader: Read,
        Progress: FnMut(u64),
        Cancelled: FnMut() -> bool,
    {
        self.connection.send_file_header(
            FILE_PAYLOAD_ID,
            wire_size,
            Some(name.into()),
        )?;
        if size == 0 {
            self.stop_if_cancelled(is_cancelled)?;
            let mut extra = [0_u8; 1];
            if reader.read(&mut extra)? != 0 {
                return Err(ProtocolError::InvalidPayload);
            }
            self.connection
                .send_file_chunk(FILE_PAYLOAD_ID, 0, &[], true)?;
            on_progress(0);
            return Ok(());
        }
        let mut buffer = vec![0_u8; FILE_CHUNK_SIZE];
        let mut offset = 0_u64;
        while offset < size {
            self.stop_if_cancelled(is_cancelled)?;
            let remaining = size - offset;
            let read_limit = usize::try_from(remaining)
                .unwrap_or(FILE_CHUNK_SIZE)
                .min(FILE_CHUNK_SIZE);
            let read = reader.read(&mut buffer[..read_limit])?;
            if read == 0 {
                return Err(ProtocolError::InvalidPayload);
            }
            let read_size = u64::try_from(read)
                .map_err(|_| ProtocolError::InvalidPayload)?;
            let next_offset = offset
                .checked_add(read_size)
                .ok_or(ProtocolError::InvalidPayload)?;
            if next_offset == size {
                let mut extra = [0_u8; 1];
                if reader.read(&mut extra)? != 0 {
                    return Err(ProtocolError::InvalidPayload);
                }
            }
            self.connection.send_file_chunk(
                FILE_PAYLOAD_ID,
                i64::try_from(offset)
                    .map_err(|_| ProtocolError::InvalidPayload)?,
                &buffer[..read],
                false,
            )?;
            offset = next_offset;
            on_progress(offset);
        }
        self.stop_if_cancelled(is_cancelled)?;
        self.connection.send_file_chunk(
            FILE_PAYLOAD_ID,
            wire_size,
            &[],
            true,
        )?;
        Ok(())
    }

    pub(in crate::protocol) fn wait_for_outgoing_completion<Cancelled>(
        &mut self,
        payload_id: i64,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Cancelled: FnMut() -> bool,
    {
        self.stop_if_cancelled(is_cancelled)?;
        self.connection
            .set_read_timeout(OUTGOING_COMPLETION_TIMEOUT)?;
        loop {
            self.stop_if_cancelled(is_cancelled)?;
            let event = match self.next_transfer_event_for(payload_id) {
                Ok(event) => event,
                Err(error) => {
                    if is_cancelled() {
                        return Err(ProtocolError::Cancelled);
                    }
                    return Err(error);
                }
            };
            if is_cancelled() {
                return Err(ProtocolError::Cancelled);
            }
            match event {
                Event::Disconnected => return Ok(()),
                Event::Bytes { bytes, .. } if frames::is_v1_control(&bytes) => {
                    if frames::is_cancel(&bytes)? {
                        return Err(ProtocolError::Cancelled);
                    }
                }
                _ => return Err(ProtocolError::InvalidPayload),
            }
        }
    }

    pub(in crate::protocol) fn next_transfer_event_for(
        &mut self,
        payload_id: i64,
    ) -> Result<Event, ProtocolError> {
        loop {
            match self.connection.receive()? {
                Event::PayloadCancelled { id, .. } if id == payload_id => {
                    return Err(ProtocolError::Cancelled);
                }
                Event::PayloadError { id, .. } if id == payload_id => {
                    return Err(ProtocolError::InvalidPayload);
                }
                Event::KeepAlive { .. }
                | Event::Upgrade { .. }
                | Event::PayloadCancelled { .. }
                | Event::PayloadError { .. } => {}
                event => return Ok(event),
            }
        }
    }

    pub(in crate::protocol) fn next_transfer_event(
        &mut self,
    ) -> Result<Event, ProtocolError> {
        loop {
            match self.connection.receive()? {
                Event::KeepAlive { .. } | Event::Upgrade { .. } => {}
                Event::Disconnected => {
                    return Err(ProtocolError::Disconnected);
                }
                Event::PayloadCancelled { .. } => {
                    return Err(ProtocolError::Cancelled);
                }
                Event::PayloadError { .. } => {
                    return Err(ProtocolError::InvalidPayload);
                }
                event => return Ok(event),
            }
        }
    }

    pub(in crate::protocol) fn receive_bytes(
        &mut self,
    ) -> Result<Vec<u8>, ProtocolError> {
        match self.next_transfer_event()? {
            Event::Bytes { bytes, .. } if frames::is_cancel(&bytes)? => {
                Err(ProtocolError::Cancelled)
            }
            Event::Bytes { bytes, .. } => Ok(bytes),
            _ => Err(ProtocolError::InvalidFrame),
        }
    }

    pub(in crate::protocol) fn stop_if_cancelled<Cancelled>(
        &mut self,
        is_cancelled: &mut Cancelled,
    ) -> Result<(), ProtocolError>
    where
        Cancelled: FnMut() -> bool,
    {
        if !is_cancelled() {
            return Ok(());
        }
        self.send_control_frame(&frames::cancel())?;
        Err(ProtocolError::Cancelled)
    }
}
