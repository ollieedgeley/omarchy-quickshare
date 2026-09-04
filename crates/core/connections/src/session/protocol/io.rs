use super::super::{Connection, ConnectionIo, Error, MAX_FRAME_LENGTH};
use prost::Message as _;
use quickshare_wire::connections::OfflineFrame;
use rand_core::{OsRng, RngCore as _};
use std::io::{Read, Write};

pub(super) fn send_plain<Stream>(
    stream: &mut Stream,
    frame: &OfflineFrame,
) -> Result<(), Error>
where
    Stream: Write,
{
    write(stream, &frame.encode_to_vec())
}

pub(super) fn receive_plain<Stream>(
    stream: &mut Stream,
) -> Result<OfflineFrame, Error>
where
    Stream: Read,
{
    Ok(OfflineFrame::decode(read(stream)?.as_slice())?)
}

pub(super) fn write<Stream>(
    stream: &mut Stream,
    bytes: &[u8],
) -> Result<(), Error>
where
    Stream: Write + ?Sized,
{
    if bytes.len() > MAX_FRAME_LENGTH {
        return Err(Error::FrameTooLarge);
    }
    let length =
        u32::try_from(bytes.len()).map_err(|_| Error::FrameTooLarge)?;
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(bytes)?;
    stream.flush()?;
    Ok(())
}

pub(super) fn read<Stream>(stream: &mut Stream) -> Result<Vec<u8>, Error>
where
    Stream: Read + ?Sized,
{
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix)?;
    let size = usize::try_from(u32::from_be_bytes(prefix))
        .map_err(|_| Error::FrameTooLarge)?;
    if size > MAX_FRAME_LENGTH {
        return Err(Error::FrameTooLarge);
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

pub(super) fn iv() -> [u8; 16] {
    let mut iv = [0; 16];
    OsRng.fill_bytes(&mut iv);
    iv
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "Connection methods split handshake, transfer, and upgrade"
)]
impl Connection {
    pub(super) fn send(&mut self, frame: &OfflineFrame) -> Result<(), Error> {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), iv())
            .map_err(|_| Error::Crypto)?;
        write(&mut self.stream, &encrypted)
    }

    pub(super) fn send_on(
        &mut self,
        stream: &mut dyn ConnectionIo,
        frame: &OfflineFrame,
    ) -> Result<(), Error> {
        let encrypted = self
            .channel
            .encrypt(&frame.encode_to_vec(), iv())
            .map_err(|_| Error::Crypto)?;
        write(stream, &encrypted)
    }

    pub(super) fn recv(&mut self) -> Result<OfflineFrame, Error> {
        let bytes = self
            .channel
            .decrypt(&read(&mut self.stream)?)
            .map_err(|_| Error::Crypto)?;
        Ok(OfflineFrame::decode(bytes.as_slice())?)
    }

    pub(super) fn recv_on(
        &mut self,
        stream: &mut dyn ConnectionIo,
    ) -> Result<OfflineFrame, Error> {
        let bytes = self
            .channel
            .decrypt(&read(stream)?)
            .map_err(|_| Error::Crypto)?;
        Ok(OfflineFrame::decode(bytes.as_slice())?)
    }
}
