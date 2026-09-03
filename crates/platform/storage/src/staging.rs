use crate::Error;
use core::sync::atomic::{AtomicU64, Ordering};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
    path::{Component, Path, PathBuf},
    process,
};

/// Serial used to avoid staging-name reuse within one process.
static NEXT_STAGING_FILE: AtomicU64 = AtomicU64::new(0);
/// Maximum exclusive-create collisions before failing.
const STAGING_ATTEMPTS: u8 = 64;

/// A directory where accepted inbound files are safely completed.
#[derive(Clone, Debug)]
pub struct ReceiveTarget {
    /// Directory that owns staging and completed files.
    directory: PathBuf,
}

/// A same-directory incomplete file that can be committed once.
#[derive(Debug)]
pub struct StagedFile {
    /// Whether the staging path was removed after publication.
    committed: bool,
    /// Final path published without replacement.
    destination: PathBuf,
    /// Open staging file receiving bytes.
    file: File,
    /// Hidden path removed unless publication completes.
    staging: PathBuf,
}

impl ReceiveTarget {
    /// Resolves the user's configured downloads directory.
    ///
    /// # Errors
    ///
    /// Returns an error when no configured downloads location is available.
    #[inline]
    pub fn downloads() -> Result<Self, Error> {
        let directory = env::var_os("XDG_DOWNLOAD_DIR")
            .or_else(|| {
                env::var_os("HOME").map(|home| {
                    PathBuf::from(home).join("Downloads").into_os_string()
                })
            })
            .map(PathBuf::from)
            .ok_or(Error::InvalidSource)?;
        Ok(Self::new(directory))
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
        }
    }

    /// Creates one hidden unique file in this target directory.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is unsafe or staging cannot be created.
    #[inline]
    pub fn stage(&self, name: &str) -> Result<StagedFile, Error> {
        let path = Path::new(name);
        let safe_path =
            match (path.components().next(), path.components().nth(1)) {
                (Some(Component::Normal(_)), None) => path,
                _ => return Err(Error::InvalidName),
            };
        let destination = self.directory.join(safe_path);
        if destination.exists() {
            return Err(Error::DestinationExists);
        }
        let (file, staging) = create_staging_file(&self.directory)?;
        Ok(StagedFile {
            committed: false,
            destination,
            file,
            staging,
        })
    }
}

impl StagedFile {
    /// Flushes, syncs, and publishes this file without replacing a destination.
    ///
    /// # Errors
    ///
    /// Returns an error when syncing or publishing the file fails.
    #[inline]
    pub fn commit(mut self) -> Result<PathBuf, Error> {
        self.file.flush()?;
        self.file.sync_all()?;
        fs::hard_link(&self.staging, &self.destination).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Error::DestinationExists
            } else {
                Error::Io(error)
            }
        })?;
        self.committed = true;
        fs::remove_file(&self.staging)?;
        Ok(self.destination.clone())
    }

    /// Writes all bytes into the incomplete file.
    ///
    /// # Errors
    ///
    /// Returns an error when the local filesystem rejects the write.
    #[inline]
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.file.write_all(bytes).map_err(Error::from)
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
    }
}

/// Creates one exclusive staging file after bounded collision retries.
#[expect(
    clippy::single_call_fn,
    reason = "Staging creation remains separate from receive-name validation"
)]
fn create_staging_file(directory: &Path) -> Result<(File, PathBuf), Error> {
    for _ in 0..STAGING_ATTEMPTS {
        let serial = NEXT_STAGING_FILE.fetch_add(1, Ordering::Relaxed);
        let staging = directory
            .join(format!(".quickshare-{}-{serial}.part", process::id()));
        match OpenOptions::new()
            .create_new(true)
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
