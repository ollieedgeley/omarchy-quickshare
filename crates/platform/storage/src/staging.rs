extern crate alloc;

use crate::{Error, path::safe_file_name, quota};
use alloc::{collections::BTreeSet, sync::Arc};
use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, IoSlice, Write},
    os::unix::fs::OpenOptionsExt as _,
    path::{Path, PathBuf},
    process,
    sync::{Mutex, PoisonError},
};

/// Bounded retries when creating a unique hidden staging file.
const STAGING_ATTEMPTS: u8 = 64;
/// Linux `O_NOFOLLOW`; reject a final-path symlink without following it.
const O_NOFOLLOW: i32 = 0o400_000;
/// Monotonic suffix for unique `.quickshare-*.part` staging names.
static NEXT_STAGING_FILE: AtomicU64 = AtomicU64::new(0);

/// A directory where accepted inbound files are safely completed.
#[derive(Clone, Debug)]
pub struct ReceiveTarget {
    /// Directory that owns staging and completed files.
    directory: PathBuf,
    /// In-flight destination names reserved by this target.
    reserved: Arc<Mutex<BTreeSet<String>>>,
}

/// A same-directory incomplete file that can be committed once.
#[derive(Debug)]
pub struct StagedFile {
    /// Whether the staging path was removed after publication.
    committed: bool,
    /// Exact byte count the attachment must contain.
    declared_size: u64,
    /// Final path published without replacement.
    destination: PathBuf,
    /// Open staging file receiving bytes.
    file: File,
    /// Basename reserved until this staging file is dropped.
    name: String,
    /// Shared in-flight destination names for this receive root.
    reserved: Arc<Mutex<BTreeSet<String>>>,
    /// Hidden path removed unless publication completes.
    staging: PathBuf,
    /// Bytes accepted into the staging file.
    written: u64,
}

impl ReceiveTarget {
    /// Resolves the default `Downloads/omarchy-quickshare` receive root.
    ///
    /// # Errors
    ///
    /// Returns an error when no configured downloads location is available.
    #[inline]
    pub fn downloads() -> Result<Self, Error> {
        let directory = env::var_os("XDG_DOWNLOAD_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join("Downloads"))
            })
            .ok_or(Error::InvalidSource)?;
        Self::open(directory.join("omarchy-quickshare"))
    }

    /// Creates a target for an explicit directory for controlled use.
    #[must_use]
    #[inline]
    pub fn new<Directory>(directory: Directory) -> Self
    where
        Directory: Into<PathBuf>,
    {
        Self {
            directory: directory.into(),
            reserved: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    /// Opens `directory` as a configured receive root, creating it if needed.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created or is not a
    /// directory.
    #[inline]
    pub fn open<Directory>(directory: Directory) -> Result<Self, Error>
    where
        Directory: AsRef<Path>,
    {
        let path = directory.as_ref();
        fs::create_dir_all(path)?;
        let resolved_directory = fs::canonicalize(path)?;
        if !fs::metadata(&resolved_directory)?.is_dir() {
            return Err(Error::InvalidSource);
        }
        Ok(Self::new(resolved_directory))
    }

    /// Rejects the share when `bytes` cannot fit on this receive root.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Quota`] when free space is below `bytes`.
    #[inline]
    pub fn preflight(&self, bytes: u64) -> Result<(), Error> {
        quota::preflight(&self.directory, bytes)
    }

    /// Reserves `name` when the destination is free.
    fn reserve(&self, name: &str, destination: &Path) -> Result<(), Error> {
        if destination.exists() {
            return Err(Error::Collision);
        }
        let mut reserved =
            self.reserved.lock().unwrap_or_else(PoisonError::into_inner);
        let inserted = reserved.insert(name.to_owned());
        drop(reserved);
        if !inserted {
            return Err(Error::Collision);
        }
        Ok(())
    }

    /// Creates one hidden unique file for `name` with an exact declared size.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is unsafe, collides, or staging cannot be
    /// created.
    #[inline]
    pub fn stage(
        &self,
        name: &str,
        declared_size: u64,
    ) -> Result<StagedFile, Error> {
        let safe_name = safe_file_name(name)?;
        let destination = self.directory.join(safe_name);
        self.reserve(safe_name, &destination)?;
        match create_staging_file(&self.directory) {
            Ok((file, staging)) => Ok(StagedFile {
                committed: false,
                declared_size,
                destination,
                file,
                name: safe_name.to_owned(),
                reserved: Arc::clone(&self.reserved),
                staging,
                written: 0,
            }),
            Err(error) => {
                unreserve(&self.reserved, safe_name);
                Err(error)
            }
        }
    }
}

impl StagedFile {
    /// Flushes, syncs, and publishes this file without replacing a destination.
    ///
    /// # Errors
    ///
    /// Returns an error when the written size is wrong or publishing fails.
    #[inline]
    pub fn commit(mut self) -> Result<PathBuf, Error> {
        self.file.flush()?;
        self.file.sync_all()?;
        if self.written < self.declared_size {
            return Err(Error::Interrupted);
        }
        if self.written != self.declared_size {
            return Err(Error::SizeMismatch);
        }
        fs::hard_link(&self.staging, &self.destination).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Error::Collision
            } else {
                Error::Io(error)
            }
        })?;
        self.committed = true;
        fs::remove_file(&self.staging)?;
        Ok(self.destination.clone())
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Pinned Rust leaves two Write methods unstable; others delegate"
)]
impl Write for StagedFile {
    #[inline]
    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }

    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        reject_overflow(self.written, self.declared_size, buf.len())?;
        let wrote = self.file.write(buf)?;
        self.written = add_written(self.written, wrote)?;
        Ok(wrote)
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        reject_overflow(self.written, self.declared_size, buf.len())?;
        self.file.write_all(buf)?;
        self.written = add_written(self.written, buf.len())?;
        Ok(())
    }

    #[inline]
    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> io::Result<()> {
        self.write_all(args.to_string().as_bytes())
    }

    #[inline]
    fn write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> io::Result<usize> {
        let total = bufs
            .iter()
            .try_fold(0_usize, |sum, buf| sum.checked_add(buf.len()))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "declared size exceeded",
                )
            })?;
        reject_overflow(self.written, self.declared_size, total)?;
        let wrote = self.file.write_vectored(bufs)?;
        self.written = add_written(self.written, wrote)?;
        Ok(wrote)
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for StagedFile {
    #[inline]
    fn drop(&mut self) {
        if !self.committed {
            drop(fs::remove_file(&self.staging));
        }
        unreserve(&self.reserved, &self.name);
    }
}

/// Creates one exclusive staging file after bounded collision retries.
#[expect(
    clippy::single_call_fn,
    reason = "keeps exclusive staging creation and bounded retry atomic"
)]
fn create_staging_file(directory: &Path) -> Result<(File, PathBuf), Error> {
    for _ in 0..STAGING_ATTEMPTS {
        let serial = NEXT_STAGING_FILE.fetch_add(1, Ordering::Relaxed);
        let staging = directory
            .join(format!(".quickshare-{}-{serial}.part", process::id()));
        match OpenOptions::new()
            .create_new(true)
            .custom_flags(O_NOFOLLOW)
            .write(true)
            .open(&staging)
        {
            Ok(file) => return Ok((file, staging)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(
        io::Error::new(io::ErrorKind::AlreadyExists, "staging collision")
            .into(),
    )
}

/// Adds `additional` accepted bytes onto `written`.
fn add_written(written: u64, additional: usize) -> io::Result<u64> {
    let converted = u64::try_from(additional).map_err(io::Error::other)?;
    written.checked_add(converted).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "declared size exceeded")
    })
}

/// Rejects a write that would exceed the attachment's declared size.
fn reject_overflow(
    written: u64,
    declared_size: u64,
    additional: usize,
) -> io::Result<()> {
    let total = add_written(written, additional)?;
    if total > declared_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "declared size exceeded",
        ));
    }
    Ok(())
}

/// Drops an in-flight destination reservation.
fn unreserve(reserved: &Mutex<BTreeSet<String>>, name: &str) {
    let _removed = reserved
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(name);
}
