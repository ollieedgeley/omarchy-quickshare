use core::sync::atomic::{AtomicU64, Ordering};
use std::{
    collections::HashSet,
    env,
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Component, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use quickshare_control::response::OperationRecord;
use quickshare_sharing::Phase;
use rustix::{
    fs::{self, AtFlags, Mode, OFlags},
    process,
};
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "operations.json";
const TERMINAL_LIMIT: usize = 128;
const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Document {
    schema_version: u32,
    next_id: u64,
    records: Vec<OperationRecord>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_id: 1,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Journal {
    directory: Option<File>,
    document: Document,
    poisoned: bool,
}

impl Journal {
    pub(super) fn open() -> io::Result<Self> {
        let directory = state_directory()?;
        let mut document = load(&directory)?;
        validate(&document)?;
        for record in &mut document.records {
            if !terminal(record.phase) {
                record.phase = Phase::Failed;
                record.terminal_reason = Some("interrupted".to_owned());
                record.recovery_guidance = Some(
                    "The daemon restarted and the final outcome is \
                     unconfirmed. \
                     Content may already have been committed or delivered. \
                     Transfers cannot resume. Check the received content or \
                     ask the peer before deliberately resending."
                        .to_owned(),
                );
            }
        }
        trim(&mut document.records);
        persist(&directory, &document)?;
        Ok(Self {
            directory: Some(directory),
            document,
            poisoned: false,
        })
    }

    pub(super) fn records(&self) -> &[OperationRecord] {
        &self.document.records
    }

    pub(super) const fn next_id(&self) -> u64 {
        self.document.next_id
    }

    pub(super) fn request(&self, id: &str) -> Option<&OperationRecord> {
        self.document
            .records
            .iter()
            .find(|record| record.request_id.as_deref() == Some(id))
    }

    pub(super) fn update(
        &mut self,
        record: OperationRecord,
        next_id: u64,
    ) -> io::Result<()> {
        self.ensure_writable()?;
        if next_id < self.document.next_id {
            return Err(invalid("operation sequence moved backwards"));
        }
        if next_id == self.document.next_id {
            if self.document.records.contains(&record) {
                return Ok(());
            }
            if let Some(old) = self.document.records.iter_mut().find(|old| {
                old.id_string == record.id_string
                    && !terminal(old.phase)
                    && old.phase == record.phase
                    && old.request_id == record.request_id
                    && old.direction == record.direction
                    && old.total_bytes == record.total_bytes
                    && old.dismissed == record.dismissed
                    && old.terminal_reason == record.terminal_reason
                    && old.recovery_guidance == record.recovery_guidance
                    && record.transferred_bytes >= old.transferred_bytes
                    && record.transferred_bytes <= record.total_bytes
            }) {
                // Progress is advisory until the next durable phase change.
                old.transferred_bytes = record.transferred_bytes;
                return Ok(());
            }
        }
        let mut candidate = self.document.clone();
        if let Some(old) = candidate
            .records
            .iter_mut()
            .find(|old| old.id_string == record.id_string)
        {
            if old.request_id != record.request_id
                || old.direction != record.direction
                || old.total_bytes != record.total_bytes
                || old.transferred_bytes > record.transferred_bytes
            {
                return Err(invalid("operation identity or progress changed"));
            }
            if terminal(old.phase) {
                let mut truth = record.clone();
                truth.dismissed = old.dismissed;
                if truth != *old {
                    return Err(invalid(
                        "terminal operation cannot be changed",
                    ));
                }
            }
            *old = record;
        } else {
            if record_id(&record)? < self.document.next_id {
                return Err(invalid(
                    "operation identifier was already allocated",
                ));
            }
            candidate.records.push(record);
        }
        candidate.next_id = next_id;
        // Validate before eviction so a duplicate identity cannot hide in an
        // entry about to leave the bounded terminal history.
        validate_records(&candidate)?;
        trim(&mut candidate.records);
        self.commit(candidate)
    }

    pub(super) fn dismiss(&mut self, id: u64) -> io::Result<bool> {
        self.ensure_writable()?;
        let id = id.to_string();
        let mut candidate = self.document.clone();
        let Some(record) = candidate
            .records
            .iter_mut()
            .find(|record| record.id_string == id && terminal(record.phase))
        else {
            return Ok(false);
        };
        if record.dismissed {
            return Ok(true);
        }
        record.dismissed = true;
        self.commit(candidate)?;
        Ok(true)
    }

    fn commit(&mut self, candidate: Document) -> io::Result<()> {
        validate(&candidate)?;
        if let Some(directory) = &self.directory
            && let Err(error) = persist(directory, &candidate)
        {
            self.poisoned = true;
            return Err(error);
        }
        self.document = candidate;
        Ok(())
    }

    fn ensure_writable(&self) -> io::Result<()> {
        if self.poisoned {
            return Err(io::Error::other(
                "operation journal persistence failed; \
                 restart before accepting changes",
            ));
        }
        Ok(())
    }
}

const fn terminal(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::Cancelled | Phase::Completed | Phase::Failed | Phase::Rejected
    )
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn record_id(record: &OperationRecord) -> io::Result<u64> {
    let text = &record.id_string;
    if text.is_empty()
        || text.starts_with('0')
        || !text.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid(
            "operation identifier is not canonical nonzero decimal",
        ));
    }
    text.parse()
        .map_err(|_| invalid("operation identifier exceeds u64"))
}

fn validate(document: &Document) -> io::Result<()> {
    if document.records.len() > TERMINAL_LIMIT + 1
        || document
            .records
            .iter()
            .filter(|record| terminal(record.phase))
            .count()
            > TERMINAL_LIMIT
    {
        return Err(invalid("operation journal exceeds history limit"));
    }
    validate_records(document)
}

fn validate_records(document: &Document) -> io::Result<()> {
    if document.schema_version != 1 || document.next_id == 0 {
        return Err(invalid(
            "unsupported journal schema or zero operation sequence",
        ));
    }
    let mut ids = HashSet::new();
    let mut requests = HashSet::new();
    let mut active = 0;
    for record in &document.records {
        let id = record_id(record)?;
        if id >= document.next_id || !ids.insert(id) {
            return Err(invalid(
                "duplicate or unallocated operation identifier",
            ));
        }
        if let Some(request) = &record.request_id
            && (request.is_empty()
                || request.len() > 128
                || !requests.insert(request))
        {
            return Err(invalid("invalid or duplicate request identifier"));
        }
        if record.transferred_bytes > record.total_bytes
            || record
                .terminal_reason
                .as_ref()
                .is_some_and(|text| text.len() > 128)
            || record
                .recovery_guidance
                .as_ref()
                .is_some_and(|text| text.len() > 1024)
        {
            return Err(invalid("invalid operation progress or outcome"));
        }
        if !terminal(record.phase) {
            active += 1;
            if record.dismissed
                || record.terminal_reason.is_some()
                || record.recovery_guidance.is_some()
            {
                return Err(invalid(
                    "active operation contains a terminal outcome",
                ));
            }
        }
    }
    if active > 1 {
        return Err(invalid(
            "operation journal contains multiple active operations",
        ));
    }
    Ok(())
}

fn trim(records: &mut Vec<OperationRecord>) {
    while records
        .iter()
        .filter(|record| terminal(record.phase))
        .count()
        > TERMINAL_LIMIT
    {
        if let Some(index) =
            records.iter().position(|record| terminal(record.phase))
        {
            let _removed = records.remove(index);
        }
    }
}

fn state_directory() -> io::Result<File> {
    let base = match env::var_os("XDG_STATE_HOME") {
        Some(value) => absolute_base(PathBuf::from(value))?,
        None => {
            let home = env::var_os("HOME")
                .ok_or_else(|| invalid("HOME is not set"))?;
            absolute_base(PathBuf::from(home))?.join(".local/state")
        }
    };
    let path = base.join("omarchy-quickshare");
    let mut directory = File::from(fs::open(
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                match fs::mkdirat(&directory, name, Mode::RWXU) {
                    Ok(()) => directory.sync_all()?,
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(error) => return Err(error.into()),
                }
                directory = File::from(fs::openat(
                    &directory,
                    name,
                    OFlags::RDONLY
                        | OFlags::DIRECTORY
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )?);
            }
            _ => {
                return Err(invalid(
                    "state directory contains unsafe path components",
                ));
            }
        }
    }
    let metadata = directory.metadata()?;
    if metadata.uid() != process::geteuid().as_raw() {
        return Err(invalid("operation journal directory has another owner"));
    }
    fs::fchmod(&directory, Mode::RWXU)?;
    directory.sync_all()?;
    Ok(directory)
}

fn absolute_base(path: PathBuf) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(invalid(
            "state directory base must be nonempty and absolute",
        ));
    }
    Ok(path)
}

fn checked_file(directory: &File) -> io::Result<Option<File>> {
    let file = match fs::openat(
        directory,
        FILE_NAME,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => File::from(fd),
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() > MAX_DOCUMENT_BYTES
    {
        return Err(invalid("operation journal is not a private regular file"));
    }
    Ok(Some(file))
}

fn load(directory: &File) -> io::Result<Document> {
    let Some(file) = checked_file(directory)? else {
        return Ok(Document::default());
    };
    let mut bytes = Vec::new();
    let _count = file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() > usize::try_from(MAX_DOCUMENT_BYTES).unwrap_or(usize::MAX) {
        return Err(invalid("operation journal exceeds size limit"));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| invalid("operation journal is corrupt"))
}

fn persist(directory: &File, document: &Document) -> io::Result<()> {
    let _existing = checked_file(directory)?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = format!(
        ".operations-{}-{timestamp}-{sequence}.tmp",
        std::process::id()
    );
    let mut file = File::from(fs::openat(
        directory,
        temporary.as_str(),
        OFlags::WRONLY
            | OFlags::CREATE
            | OFlags::EXCL
            | OFlags::NOFOLLOW
            | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?);
    let result = (|| {
        fs::fchmod(&file, Mode::RUSR | Mode::WUSR)?;
        serde_json::to_writer(&mut file, document).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        let _destination = checked_file(directory)?;
        fs::renameat(directory, temporary.as_str(), directory, FILE_NAME)?;
        directory.sync_all()
    })();
    if result.is_err() {
        let _cleanup =
            fs::unlinkat(directory, temporary.as_str(), AtFlags::empty());
    }
    result
}
