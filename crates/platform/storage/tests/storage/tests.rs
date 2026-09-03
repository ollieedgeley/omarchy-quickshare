#![expect(
    clippy::expect_used,
    reason = "Contract tests require descriptive filesystem failures"
)]
#![expect(
    clippy::assertions_on_result_states,
    reason = "Contract tests assert expected failure states"
)]

use core::sync::atomic::{AtomicU64, Ordering};
use quickshare_storage::{OutboundSource, ReceiveTarget};
use std::{
    env, fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);
#[derive(Debug)]
struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> Self {
        let serial = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "quickshare-storage-{}-{nanos}-{serial}",
            process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "Drop has no project-implementable default methods"
)]
impl Drop for TestDirectory {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).expect("remove test directory");
    }
}

#[test]
fn outbound_source_reads_a_regular_file_with_its_basename() {
    let directory = TestDirectory::new();
    let source_path = directory.path().join("report.txt");
    fs::write(&source_path, b"hello").expect("write source");

    let source = OutboundSource::open(&source_path).expect("open source");

    assert_eq!(source.name(), Path::new("report.txt").as_os_str());
    assert_eq!(source.len(), 5);
    let mut reader = source.reader().expect("source reader");
    let mut bytes = [0_u8; 5];
    reader.read_exact(&mut bytes).expect("read source");
    assert_eq!(bytes, *b"hello");
}

#[test]
fn outbound_source_readers_have_independent_positions() {
    let directory = TestDirectory::new();
    let source_path = directory.path().join("report.txt");
    fs::write(&source_path, b"hello").expect("write source");
    let source = OutboundSource::open(&source_path).expect("open source");
    let mut first = source.reader().expect("first source reader");
    let mut second = source.reader().expect("second source reader");

    let mut first_byte = [0_u8; 1];
    first.read_exact(&mut first_byte).expect("read first");
    let mut complete = [0_u8; 5];
    second.read_exact(&mut complete).expect("read second");

    assert_eq!(first_byte, *b"h");
    assert_eq!(complete, *b"hello");
}

#[test]
fn outbound_source_rejects_directories_and_changed_lengths() {
    let directory = TestDirectory::new();
    assert!(OutboundSource::open(directory.path()).is_err());

    let source_path = directory.path().join("report.txt");
    fs::write(&source_path, b"before").expect("write source");
    let source = OutboundSource::open(&source_path).expect("open source");
    fs::write(&source_path, b"afterwards").expect("change source");

    assert!(source.reader().is_err());
}

#[test]
fn receive_target_commits_a_complete_file_without_replacing_a_destination() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::new(directory.path());
    let mut staged = target.stage("received.txt").expect("stage file");
    staged.write_all(b"complete").expect("write staged file");

    let committed = staged.commit().expect("commit staged file");

    assert_eq!(committed, directory.path().join("received.txt"));
    assert_eq!(
        fs::read(committed).expect("read committed file"),
        b"complete"
    );
    assert!(target.stage("received.txt").is_err());
}

#[test]
fn receive_target_rejects_unsafe_names_and_cleans_uncommitted_files() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::new(directory.path());
    for name in ["", ".", "..", "nested/name", "/absolute"] {
        assert!(target.stage(name).is_err(), "accepted {name:?}");
    }

    let mut staged = target.stage("discard.txt").expect("stage file");
    staged.write_all(b"discard").expect("write staged file");
    drop(staged);

    assert!(directory_is_empty(directory.path()));
}

#[expect(
    clippy::single_call_fn,
    reason = "The cleanup assertion is named for its filesystem contract"
)]
fn directory_is_empty(path: &Path) -> bool {
    fs::read_dir(path).expect("read directory").next().is_none()
}
