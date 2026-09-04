//! Temporary ZIP archives for folder shares.

use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

/// Creates a store-only ZIP of `source` in a unique temporary file.
///
/// # Errors
///
/// Returns an error when `source` is not a directory or cannot be archived.
#[must_use]
pub(crate) fn zip_directory(source: &Path) -> io::Result<PathBuf> {
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder sharing requires a directory",
        ));
    }
    let name = source.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder path has no directory name",
        )
    })?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let archive = env_temp().join(format!(
        "omarchy-quickshare-{}-{nanos}-{}.zip",
        process::id(),
        name.to_string_lossy()
    ));
    write_zip(source, &archive).inspect_err(|_error| {
        drop(fs::remove_file(&archive));
    })?;
    Ok(archive)
}

/// Removes a temporary folder archive after the share ends.
pub(crate) fn remove_archive(path: &Path) {
    drop(fs::remove_file(path));
}

/// Writes one store-only archive for every regular file under `source`.
fn write_zip(source: &Path, archive: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    collect_files(source, source, &mut files)?;
    let mut output = File::create(archive)?;
    let mut directory = Vec::new();
    let mut offset = 0_u32;
    let mut buffer = [0_u8; 8192];
    for (relative, absolute) in &files {
        offset = append_stored_file(
            &mut output,
            relative,
            absolute,
            &mut buffer,
            offset,
            &mut directory,
        )?;
    }
    let directory_start = offset;
    for entry in &directory {
        write_central(&mut output, entry)?;
    }
    let directory_size = offset_after_directory(&directory)?;
    write_eocd(
        &mut output,
        u16::try_from(directory.len()).map_err(|_error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder has too many files to archive",
            )
        })?,
        directory_size,
        directory_start,
    )
}

/// Writes one stored file using a bounded CRC pass then a streaming copy.
#[expect(
    clippy::single_call_fn,
    reason = "Per-entry store writing stays out of archive finalization"
)]
fn append_stored_file(
    output: &mut File,
    relative: &Path,
    absolute: &Path,
    buffer: &mut [u8],
    offset: u32,
    directory: &mut Vec<Central>,
) -> io::Result<u32> {
    let mut input = File::open(absolute)?;
    let (crc, size) = crc32_and_size(&mut input, buffer)?;
    let _reset = input.seek(SeekFrom::Start(0))?;
    let name = zip_name(relative)?;
    let name_len = u16::try_from(name.len()).map_err(|_error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder entry name is too long",
        )
    })?;
    write_local(output, &name, name_len, crc, size)?;
    let copied = io::copy(&mut input, output)?;
    if copied != u64::from(size) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder entry changed while archiving",
        ));
    }
    directory.push(Central {
        crc,
        name,
        name_len,
        offset,
        size,
    });
    offset
        .checked_add(30)
        .and_then(|value| value.checked_add(u32::from(name_len)))
        .and_then(|value| value.checked_add(size))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder archive exceeded 4 GiB",
            )
        })
}

/// One central-directory record for a stored file.
struct Central {
    /// CRC-32 of the uncompressed bytes.
    crc: u32,
    /// Archive-relative path using `/` separators.
    name: Vec<u8>,
    /// Length of `name`.
    name_len: u16,
    /// Local-header offset from the start of the archive.
    offset: u32,
    /// Stored size, equal to the uncompressed size.
    size: u32,
}

/// Recursively collects regular files while rejecting path escape.
fn collect_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder sharing rejects symbolic links",
            ));
        }
        if metadata.is_dir() {
            collect_files(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        let relative = path.strip_prefix(root).map_err(|_error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder entry escaped the source directory",
            )
        })?;
        files.push((relative.to_path_buf(), path));
    }
    Ok(())
}

/// Encodes a relative path as a ZIP name without `.` or `..` components.
fn zip_name(relative: &Path) -> io::Result<Vec<u8>> {
    let mut name = String::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder entry has an unsafe path",
            ));
        };
        if !name.is_empty() {
            name.push('/');
        }
        name.push_str(&part.to_string_lossy());
    }
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder entry has no file name",
        ));
    }
    Ok(name.into_bytes())
}

/// Writes one local file header.
fn write_local(
    output: &mut File,
    name: &[u8],
    name_len: u16,
    crc: u32,
    size: u32,
) -> io::Result<()> {
    output.write_all(&0x0403_4b50_u32.to_le_bytes())?;
    output.write_all(&20_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&crc.to_le_bytes())?;
    output.write_all(&size.to_le_bytes())?;
    output.write_all(&size.to_le_bytes())?;
    output.write_all(&name_len.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(name)
}

/// Writes one central directory header.
fn write_central(output: &mut File, entry: &Central) -> io::Result<()> {
    output.write_all(&0x0201_4b50_u32.to_le_bytes())?;
    output.write_all(&20_u16.to_le_bytes())?;
    output.write_all(&20_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&entry.crc.to_le_bytes())?;
    output.write_all(&entry.size.to_le_bytes())?;
    output.write_all(&entry.size.to_le_bytes())?;
    output.write_all(&entry.name_len.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u32.to_le_bytes())?;
    output.write_all(&entry.offset.to_le_bytes())?;
    output.write_all(&entry.name)
}

/// Writes the end-of-central-directory record.
fn write_eocd(
    output: &mut File,
    count: u16,
    directory_size: u32,
    directory_start: u32,
) -> io::Result<()> {
    output.write_all(&0x0605_4b50_u32.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())?;
    output.write_all(&count.to_le_bytes())?;
    output.write_all(&count.to_le_bytes())?;
    output.write_all(&directory_size.to_le_bytes())?;
    output.write_all(&directory_start.to_le_bytes())?;
    output.write_all(&0_u16.to_le_bytes())
}

/// Returns the central-directory byte length.
fn offset_after_directory(directory: &[Central]) -> io::Result<u32> {
    let mut size = 0_u32;
    for entry in directory {
        size = size
            .checked_add(46)
            .and_then(|value| value.checked_add(u32::from(entry.name_len)))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "folder archive exceeded 4 GiB",
                )
            })?;
    }
    Ok(size)
}

/// Updates an IEEE CRC-32 over the next chunk.
fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Computes CRC-32 and size without buffering the whole file.
#[expect(
    clippy::single_call_fn,
    reason = "CRC and size are computed before the stored payload is copied"
)]
fn crc32_and_size(
    input: &mut File,
    buffer: &mut [u8],
) -> io::Result<(u32, u32)> {
    let mut crc = 0xFFFF_FFFF_u32;
    let mut size = 0_u64;
    loop {
        let read = input.read(buffer)?;
        if read == 0 {
            break;
        }
        crc = crc32_update(crc, &buffer[..read]);
        let read = u32::try_from(read).map_err(|_error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder is too large to archive",
            )
        })?;
        size = size.checked_add(u64::from(read)).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "folder is too large to archive",
            )
        })?;
    }
    let size = u32::try_from(size).map_err(|_error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "folder is too large to archive",
        )
    })?;
    Ok((!crc, size))
}

/// Returns the process temporary directory.
fn env_temp() -> PathBuf {
    std::env::temp_dir()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "Focused unit tests stay beside private ZIP encoding"
)]
mod tests {
    use std::fs::{self, File};
    use std::io::Read;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::process;

    use super::{crc32_update, remove_archive, zip_directory};

    fn crc32(bytes: &[u8]) -> u32 {
        !crc32_update(0xFFFF_FFFF, bytes)
    }

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "omarchy-quickshare-archive-{}-{name}",
            process::id()
        ));
        drop(fs::remove_dir_all(&root));
        fs::create_dir_all(&root).expect("test root");
        root
    }

    fn stored_entry(bytes: &[u8]) -> (u32, u32, &[u8], &[u8]) {
        assert_eq!(&bytes[0..4], [0x50, 0x4b, 0x03, 0x04]);
        assert_eq!(&bytes[8..10], [0, 0]);
        let crc = u32::from_le_bytes(bytes[14..18].try_into().expect("crc"));
        let size = u32::from_le_bytes(bytes[18..22].try_into().expect("size"));
        let uncompressed =
            u32::from_le_bytes(bytes[22..26].try_into().expect("usize"));
        assert_eq!(size, uncompressed);
        let name_len = usize::from(u16::from_le_bytes(
            bytes[26..28].try_into().expect("name"),
        ));
        let extra =
            u16::from_le_bytes(bytes[28..30].try_into().expect("extra"));
        assert_eq!(extra, 0);
        let data_size = usize::try_from(size).expect("size");
        let name = &bytes[30..30 + name_len];
        let data = &bytes[30 + name_len..30 + name_len + data_size];
        (crc, size, name, data)
    }

    #[test]
    fn folder_archive_stores_file_bytes_with_crc_and_size() {
        let root = fixture("store");
        fs::write(root.join("note.txt"), b"hi").expect("file");
        let archive = zip_directory(&root).expect("zip");
        let mut bytes = Vec::new();
        let _read = File::open(&archive)
            .expect("open")
            .read_to_end(&mut bytes)
            .expect("read");
        let (crc, size, name, data) = stored_entry(&bytes);
        assert_eq!(name, b"note.txt");
        assert_eq!(data, b"hi");
        assert_eq!(size, 2);
        assert_eq!(crc, crc32(b"hi"));
        remove_archive(&archive);
        drop(fs::remove_dir_all(&root));
    }

    #[test]
    fn chunked_crc_matches_whole_buffer_crc() {
        let data = vec![0xAB_u8; 70_000];
        let mut crc = 0xFFFF_FFFF_u32;
        for chunk in data.chunks(8192) {
            crc = crc32_update(crc, chunk);
        }
        assert_eq!(!crc, crc32(&data));
    }

    #[test]
    fn large_file_archive_keeps_store_crc_and_payload() {
        let root = fixture("large");
        let payload = vec![0xCD_u8; 20_000];
        fs::write(root.join("clip.bin"), &payload).expect("file");
        let archive = zip_directory(&root).expect("zip");
        let mut bytes = Vec::new();
        let _read = File::open(&archive)
            .expect("open")
            .read_to_end(&mut bytes)
            .expect("read");
        let (crc, size, name, data) = stored_entry(&bytes);
        assert_eq!(name, b"clip.bin");
        assert_eq!(data, payload.as_slice());
        assert_eq!(size, u32::try_from(payload.len()).expect("size"));
        assert_eq!(crc, crc32(&payload));
        remove_archive(&archive);
        drop(fs::remove_dir_all(&root));
    }

    #[test]
    fn folder_archive_rejects_symbolic_links() {
        let root = fixture("symlink");
        fs::write(root.join("note.txt"), b"hi").expect("file");
        symlink(root.join("note.txt"), root.join("link.txt")).expect("link");
        let error = zip_directory(&root).expect_err("symlink");
        assert!(error.to_string().contains("symbolic links"));
        drop(fs::remove_dir_all(&root));
    }
}
