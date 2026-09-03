use crate::Error;
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io::{self, Read},
    os::unix::fs::FileExt as _,
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

/// One independently positioned view of an accepted source descriptor.
#[derive(Debug)]
struct SourceReader {
    /// Descriptor read without mutating its shared file cursor.
    file: File,
    /// Next byte offset owned only by this reader.
    position: u64,
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Default Read methods preserve this positional reader's semantics"
)]
impl Read for SourceReader {
    #[inline]
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.file.read_at(buf, self.position)?;
        let advance = u64::try_from(read).map_err(io::Error::other)?;
        self.position =
            self.position.checked_add(advance).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source offset overflow",
                )
            })?;
        Ok(read)
    }
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

    /// Opens an independently positioned reader while the length is unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be cloned or changed length.
    #[inline]
    pub fn reader(&self) -> Result<impl Read, Error> {
        if self.file.metadata()?.len() != self.length {
            return Err(Error::SourceChanged);
        }
        Ok(SourceReader {
            file: self.file.try_clone()?,
            position: 0,
        })
    }
}
