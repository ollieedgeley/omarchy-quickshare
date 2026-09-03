use super::super::{Error, MAX_FRAME_LENGTH};
use prost::Message as _;
use quickshare_wire::connections::OfflineFrame;
use rand_core::{OsRng, RngCore as _};
use std::{
    io::{Read as _, Write as _},
    net::TcpStream,
};

pub(super) fn send_plain(
    stream: &mut TcpStream,
    frame: &OfflineFrame,
) -> Result<(), Error> {
    write(stream, &frame.encode_to_vec())
}

pub(super) fn receive_plain(
    stream: &mut TcpStream,
) -> Result<OfflineFrame, Error> {
    Ok(OfflineFrame::decode(read(stream)?.as_slice())?)
}

pub(super) fn write(stream: &mut TcpStream, bytes: &[u8]) -> Result<(), Error> {
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

pub(super) fn read(stream: &mut TcpStream) -> Result<Vec<u8>, Error> {
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
