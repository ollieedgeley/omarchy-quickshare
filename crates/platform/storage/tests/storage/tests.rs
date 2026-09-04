#![expect(
    clippy::expect_used,
    reason = "Contract tests require descriptive filesystem failures"
)]

use core::sync::atomic::{AtomicU64, Ordering};
use quickshare_storage::{Error, OutboundSource, ReceiveTarget};
use std::{
    env, fs,
    io::{Read as _, Write as _},
    os::unix::fs::symlink,
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
fn outbound_source_rejects_directories_symlinks_and_mutation() {
    let directory = TestDirectory::new();
    assert!(matches!(
        OutboundSource::open(directory.path()),
        Err(Error::InvalidSource)
    ));

    let source_path = directory.path().join("report.txt");
    fs::write(&source_path, b"before").expect("write source");
    let mutated = OutboundSource::open(&source_path).expect("open source");
    fs::write(&source_path, b"afterwards").expect("change source");
    assert!(matches!(mutated.reader(), Err(Error::Mutation)));

    fs::write(&source_path, b"hello").expect("rewrite source");
    let replaced = OutboundSource::open(&source_path).expect("open source");
    fs::remove_file(&source_path).expect("remove source");
    fs::write(&source_path, b"hello").expect("replace source");
    assert!(matches!(replaced.reader(), Err(Error::Mutation)));

    let link = directory.path().join("link.txt");
    symlink(&source_path, &link).expect("symlink source");
    assert!(matches!(
        OutboundSource::open(&link),
        Err(Error::InvalidSource)
    ));
}

#[test]
fn receive_target_commits_a_complete_file_without_replacing_a_destination() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::open(directory.path()).expect("open root");
    target.preflight(8).expect("preflight");
    let mut staged = target.stage("received.txt", 8).expect("stage file");
    staged.write_all(b"complete").expect("write staged file");

    let committed = staged.commit().expect("commit staged file");

    assert_eq!(
        committed,
        fs::canonicalize(directory.path())
            .expect("canonicalize root")
            .join("received.txt")
    );
    assert_eq!(
        fs::read(committed).expect("read committed file"),
        b"complete"
    );
    assert!(matches!(
        target.stage("received.txt", 8),
        Err(Error::Collision)
    ));
}

#[test]
fn receive_target_rejects_hostile_names_and_cleans_interrupted_files() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::new(directory.path());
    for name in [
        "",
        ".",
        "..",
        "nested/name",
        "/absolute",
        "slash\\name",
        "new\nline",
        ".quickshare-1-0.part",
    ] {
        assert!(
            matches!(target.stage(name, 1), Err(Error::InvalidName)),
            "accepted {name:?}"
        );
    }

    let mut staged = target.stage("discard.txt", 7).expect("stage file");
    staged.write_all(b"discard").expect("write staged file");
    drop(staged);

    assert!(directory_is_empty(directory.path()));
}

#[test]
fn receive_target_enforces_declared_size_and_collision_reservations() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::new(directory.path());

    let mut oversized = target.stage("bounded.txt", 4).expect("stage bounded");
    assert!(oversized.write_all(b"12345").is_err());
    drop(oversized);

    let mut incomplete = target.stage("partial.txt", 8).expect("stage partial");
    incomplete.write_all(b"part").expect("write partial");
    assert!(matches!(incomplete.commit(), Err(Error::Interrupted)));
    assert!(!directory.path().join("partial.txt").exists());

    let first = target.stage("shared.txt", 1).expect("stage first");
    assert!(matches!(
        target.stage("shared.txt", 1),
        Err(Error::Collision)
    ));
    drop(first);

    let mut one = target.stage("a.txt", 1).expect("stage a");
    let mut two = target.stage("b.txt", 1).expect("stage b");
    one.write_all(b"a").expect("write a");
    two.write_all(b"b").expect("write b");
    drop(one.commit().expect("commit a"));
    drop(two.commit().expect("commit b"));
    assert_eq!(
        fs::read(directory.path().join("a.txt")).expect("read a"),
        b"a"
    );
    assert_eq!(
        fs::read(directory.path().join("b.txt")).expect("read b"),
        b"b"
    );
}

#[test]
fn receive_target_preflight_rejects_when_disk_cannot_fit_the_share() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::new(directory.path());
    target.preflight(0).expect("empty preflight");
    assert!(matches!(target.preflight(u64::MAX), Err(Error::Quota)));
}

#[test]
fn receive_target_preflight_reports_io_when_the_receive_root_is_unusable() {
    let directory = TestDirectory::new();
    let target = ReceiveTarget::new(directory.path().join("missing"));
    target.preflight(0).expect("empty preflight");
    assert!(
        matches!(target.preflight(1), Err(Error::Io(_))),
        "missing receive root",
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "The cleanup assertion is named for its filesystem contract"
)]
fn directory_is_empty(path: &Path) -> bool {
    fs::read_dir(path).expect("read directory").next().is_none()
}
