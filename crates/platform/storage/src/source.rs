use crate::Error;
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{Read as _, Seek as _, SeekFrom},
    path::Path,
};

/// One validated regular file offered in an outbound share.
#[derive(Debug)]
pub struct OutboundSource {
    /// Open descriptor retained to prevent path replacement races.
    file: File,
    /// Length captured when the source was accepted.
    length: u64,
    /// Basename captured when the source was accepted.
    name: OsString,
}

impl OutboundSource {
    /// Returns whether the captured source is empty.
    #[must_use]
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Returns the captured source length in bytes.
    #[must_use]
    #[inline]
    pub const fn len(&self) -> u64 {
        self.length
    }

    /// Returns the captured source basename.
    #[must_use]
    #[inline]
    pub fn name(&self) -> &OsStr {
        &self.name
    }

    /// Opens one regular file and records its basename and initial length.
    ///
    /// # Errors
    ///
    /// Returns an error when `source_path` is not a regular file or cannot be
    /// opened.
    #[inline]
    pub fn open<PathLike>(source_path: PathLike) -> Result<Self, Error>
    where
        PathLike: AsRef<Path>,
    {
        let path = source_path.as_ref();
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let name = path.file_name().ok_or(Error::InvalidSource)?;
        if !metadata.is_file() {
            return Err(Error::InvalidSource);
        }
        Ok(Self {
            file,
            length: metadata.len(),
            name: name.to_os_string(),
        })
    }

    /// Reads the source only when its length still matches the accepted file.
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be read or changed length.
    #[expect(
        clippy::verbose_file_reads,
        reason = "Retained descriptor preserves validated file identity"
    )]
    #[inline]
    pub fn read_all(&self) -> Result<Vec<u8>, Error> {
        if self.file.metadata()?.len() != self.length {
            return Err(Error::SourceChanged);
        }
        let mut file = self.file.try_clone()?;
        let _position = file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        let _bytes_read = file.read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).ok() != Some(self.length)
            || file.metadata()?.len() != self.length
        {
            return Err(Error::SourceChanged);
        }
        Ok(bytes)
    }
}
