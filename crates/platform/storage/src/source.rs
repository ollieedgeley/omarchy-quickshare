use crate::Error;
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::unix::fs::{FileExt as _, MetadataExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
};

/// Linux `O_NOFOLLOW`; reject a final-path symlink without following it.
const O_NOFOLLOW: i32 = 0o400_000;
/// Linux `ELOOP` from opening a symlink with `O_NOFOLLOW`.
const ELOOP: i32 = 40;

/// One validated regular file offered in an outbound share.
#[derive(Debug)]
pub struct OutboundSource {
    /// Device id captured when the source was accepted.
    device: u64,
    /// Open descriptor retained to prevent path replacement races.
    file: File,
    /// Inode captured when the source was accepted.
    inode: u64,
    /// Length captured when the source was accepted.
    length: u64,
    /// Basename captured when the source was accepted.
    name: OsString,
    /// Path used to detect replacement of the accepted file.
    path: PathBuf,
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
        let read =
            self.file.read_at(buf, self.position).inspect_err(|error| {
                source_io_failure("read", error);
            })?;
        tracing::trace!(
            target: "omarchy_quickshare::protocol",
            stage = "source",
            operation = "read",
            byte_offset = self.position,
            byte_count = read
        );
        let advance = u64::try_from(read).map_err(io::Error::other)?;
        self.position =
            self.position.checked_add(advance).ok_or_else(|| {
                source_failure("offset_overflow");
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

    /// Opens one regular file and records its basename and initial identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `source_path` is not a regular file or cannot be
    /// opened without following a symlink.
    #[inline]
    #[expect(
        clippy::filetype_is_file,
        reason = "the source policy rejects every non-regular inode"
    )]
    pub fn open<PathLike>(source_path: PathLike) -> Result<Self, Error>
    where
        PathLike: AsRef<Path>,
    {
        let path = source_path.as_ref();
        let listed = fs::symlink_metadata(path).inspect_err(|error| {
            source_io_failure("inspect", error);
        })?;
        if !listed.file_type().is_file() {
            source_failure("not_regular_file");
            return Err(Error::InvalidSource);
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
            .map_err(map_open_error)?;
        let metadata = file.metadata().inspect_err(|error| {
            source_io_failure("metadata", error);
        })?;
        let name = path.file_name().ok_or_else(|| {
            source_failure("missing_basename");
            Error::InvalidSource
        })?;
        if !metadata.is_file() {
            source_failure("not_regular_file");
            return Err(Error::InvalidSource);
        }
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "source",
            operation = "open",
            outcome = "success",
            byte_count = metadata.len()
        );
        Ok(Self {
            device: metadata.dev(),
            file,
            inode: metadata.ino(),
            length: metadata.len(),
            name: name.to_os_string(),
            path: path.to_path_buf(),
        })
    }

    ///
    /// # Errors
    ///
    /// Returns an error when the source cannot be cloned or was mutated.
    #[inline]
    pub fn reader(&self) -> Result<impl Read, Error> {
        self.reject_mutation()?;
        let file = self.file.try_clone().inspect_err(|error| {
            source_io_failure("clone", error);
        })?;
        tracing::debug!(
            target: "omarchy_quickshare::protocol",
            stage = "source",
            operation = "reader",
            outcome = "success",
            byte_count = self.length
        );
        Ok(SourceReader { file, position: 0 })
    }

    /// Rejects length, inode, or path replacement after acceptance.
    fn reject_mutation(&self) -> Result<(), Error> {
        let fd_meta = self.file.metadata().inspect_err(|error| {
            source_io_failure("validate_descriptor", error);
        })?;
        if fd_meta.len() != self.length
            || fd_meta.dev() != self.device
            || fd_meta.ino() != self.inode
        {
            source_failure("descriptor_mutated");
            return Err(Error::Mutation);
        }
        let path_meta =
            fs::symlink_metadata(&self.path).inspect_err(|error| {
                source_io_failure("validate_path", error);
            })?;
        if path_meta.file_type().is_symlink() {
            source_failure("source_replaced_by_symlink");
            return Err(Error::Mutation);
        }
        if path_meta.len() != self.length
            || path_meta.dev() != self.device
            || path_meta.ino() != self.inode
        {
            source_failure("path_replaced_or_mutated");
            return Err(Error::Mutation);
        }
        Ok(())
    }
}

/// Maps a symlink-loop open failure to [`Error::InvalidSource`].
#[expect(
    clippy::single_call_fn,
    reason = "isolates ELOOP mapping for platform behavior and testability"
)]
fn map_open_error(error: io::Error) -> Error {
    source_io_failure("open", &error);
    if error.raw_os_error() == Some(ELOOP) {
        source_failure("symlink");
        Error::InvalidSource
    } else {
        error.into()
    }
}
/// Emits a safe outbound source validation failure.
fn source_failure(reason: &'static str) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "source",
        operation = "validate",
        outcome = "failure",
        reason
    );
}

/// Emits a safe outbound source I/O failure.
fn source_io_failure(operation: &'static str, error: &io::Error) {
    tracing::debug!(
        target: "omarchy_quickshare::protocol",
        stage = "source",
        operation,
        outcome = "failure",
        io_error_kind = ?error.kind()
    );
}
