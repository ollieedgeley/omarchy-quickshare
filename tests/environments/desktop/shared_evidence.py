"""Record worker facts and stage bounded, no-follow desktop evidence."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import contextlib
import json
import os
import platform
import shutil
import stat
from collections.abc import Iterator
from io import BufferedReader
from pathlib import Path
from typing import Any

import build_native
import vm_control

MIB = 1024**2
GIB = 1024**3
MAX_FILES = 1024
MAX_ENTRIES = 10000
ROOT = vm_control.ROOT
CACHE = vm_control.CACHE
LOGS = {
    "desktop-prepare",
    "native-build",
    "desktop-candidate",
    "desktop-peer-prepare",
    "desktop-clean-job",
    "imagemagick",
    "host-packages",
    "toolchain",
}
PATTERNS = (
    "candidates/*/candidate.json",
    "candidates/*/build.log",
    "capacity/reference-builds/*.json",
    "preparations/*/qemu.log",
    "jobs/*/job.json",
    "jobs/*/*.log",
    "jobs/*/cleanup-evidence/*/*.json",
    "jobs/*/cleanup-evidence/*/*.log",
    "clean-installs/*/installation.json",
    "clean-installs/*/*.log",
    "clean-installs/*/commands.jsonl",
    "labs/job-*/scenarios/*/result.json",
    "labs/job-*/scenarios/*/*.log",
    "labs/job-*/scenarios/*/desktop-session.json",
    "labs/job-*/scenarios/*/ui/events.jsonl",
    "labs/job-*/scenarios/*/ui/*.png",
    "labs/job-*/scenarios/*/ui/*.tsv",
    "labs/job-*/scenarios/*/ui/*.log",
)
PREFIXES = tuple({
    str(parent)
    for pattern in PATTERNS
    for parent in Path(pattern).parents
    if parent.parts
})


def guard(path: Path) -> None:
    """Reject a redirected component before reading or mutating owned paths.

    Raises:
        RuntimeError: The path or any ancestor is a symlink.
    """
    if any(p.is_symlink() for p in (path, *path.parents)):
        message = f"Redirected path rejected: {path}"
        raise RuntimeError(message)


def directory(path: Path) -> int:
    """Return a no-follow descriptor after checking every path component."""
    guard(path)
    return os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)


def entries(descriptor: int, counter: list[int]) -> Iterator[os.DirEntry[str]]:
    """Count all entries, including unmatched names and skipped OCR rasters.

    Yields:
        Non-symlink entries while their directory descriptor remains open.

    Raises:
        RuntimeError: The traversal limit or a symlink is encountered.
    """
    with os.scandir(descriptor) as children:
        for entry in children:
            counter[0] += 1
            if counter[0] > MAX_ENTRIES:
                message = f"Evidence enumeration exceeded {MAX_ENTRIES} entries"
                raise RuntimeError(message)
            if entry.is_symlink():
                message = f"Symlink evidence entry rejected: {entry.name}"
                raise RuntimeError(message)
            yield entry


def native_inventory() -> list[str]:
    """Return the bounded pre-launch native directory inventory."""
    root = CACHE / "native"
    guard(root)
    if not root.exists():
        return []
    descriptor = directory(root)
    try:
        return [
            entry.name
            for entry in entries(descriptor, [0])
            if entry.is_dir(follow_symlinks=False)
        ]
    finally:
        os.close(descriptor)


def source_identity() -> dict[str, Any]:
    """Return the checkout revision and its explicit local worktree status."""
    return {
        "root": str(ROOT),
        "sourceCommit": vm_control.output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT
        ),
        "sourceStatus": vm_control.output(
            ["git", "status", "--porcelain=v1", "--untracked-files=all"],
            cwd=ROOT,
        ),
    }


def capture_stream(source: BufferedReader, path: Path) -> None:
    """Persist only the last MiB while draining all stage output."""
    retained = bytearray()
    while chunk := source.read1(65536):
        retained.extend(chunk)
        del retained[:-MIB]
        path.write_bytes(retained)


def filesystem_uuid(path: Path) -> str | None:
    """Identify the filesystem across subvolumes.

    Returns:
        Its UUID, or None when storage independence cannot be established.
    """
    filesystems = json.loads(
        vm_control.output(
            ["findmnt", "--json", "--target", path, "--output", "UUID"],
            timeout=10,
        )
    )["filesystems"]
    if len(filesystems) != 1:
        return None
    value = filesystems[0].get("uuid")
    return value.strip().lower() or None if isinstance(value, str) else None


def worker() -> dict[str, Any]:
    """Measure physical storage and the actual Docker/Linux worker.

    Returns:
        Worker facts and the operator's reviewed physical peak storage budgets.

    Raises:
        RuntimeError: Architecture or physical budget configuration is invalid.
    """
    if platform.machine() != "x86_64" or platform.system() != "Linux":
        message = "Linux x86_64 KVM worker required"
        raise RuntimeError(message)
    memory = dict(
        line.split(":", 1)
        for line in Path("/proc/meminfo")
        .read_text(encoding="utf-8")
        .splitlines()
    )
    docker = json.loads(
        vm_control.output(
            [
                "docker",
                "info",
                "--format",
                "{{json .}}",
            ],
            timeout=30,
        )
    )
    docker_root, workspace_gib, docker_gib, same_disk = _worker_storage(docker)
    return {
        "kernel": platform.release(),
        "os": platform.freedesktop_os_release(),
        "cpus": len(os.sched_getaffinity(0)),
        "availableMemoryMiB": int(memory["MemAvailable"].split()[0]) // 1024,
        "workspaceFreeBytes": shutil.disk_usage(ROOT).free,
        "dockerFreeBytes": shutil.disk_usage(docker_root).free,
        "dockerRoot": str(docker_root),
        "dockerVersion": docker["ServerVersion"],
        "dockerCgroupVersion": docker["CgroupVersion"],
        "sharedFilesystem": same_disk,
        "requiredWorkspaceBytes": (
            workspace_gib + (docker_gib if same_disk else 0)
        )
        * GIB,
        "requiredDockerBytes": docker_gib * GIB,
        "storagePolicy": "Reviewed physical budgets, not virtual capacity",
    }


def _worker_storage(docker: dict[str, Any]) -> tuple[Path, int, int, bool]:
    docker_root = Path(docker["DockerRootDir"])
    workspace_gib = int(os.environ["DESKTOP_WORKSPACE_BUDGET_GIB"])
    docker_gib = int(os.environ["DESKTOP_DOCKER_BUDGET_GIB"])
    if min(workspace_gib, docker_gib) < 1:
        message = "Positive reviewed physical storage budgets are required"
        raise RuntimeError(message)
    same_disk = ROOT.stat().st_dev == docker_root.stat().st_dev
    if not same_disk:
        workspace_uuid = filesystem_uuid(ROOT)
        docker_uuid = filesystem_uuid(docker_root)
        same_disk = (
            not workspace_uuid
            or not docker_uuid
            or workspace_uuid == docker_uuid
        )
    return docker_root, workspace_gib, docker_gib, same_disk


def storage(record: dict[str, Any]) -> dict[str, Any]:
    """Return physical free space and allocated blocks for the selected base."""
    guard(CACHE)
    base = CACHE / "bases" / record["baseFingerprint"] / "base.qcow2"
    guard(base)
    return {
        "freeBytes": {
            str(p): shutil.disk_usage(p).free
            for p in (ROOT, record["worker"]["dockerRoot"])
        },
        "allocatedBaseBytes": base.stat().st_blocks * 512
        if base.exists()
        else 0,
    }


def native_sources(record: dict[str, Any], counter: list[int]) -> set[Path]:
    """Return selected input and newly owned failed native attempts."""
    roots = (
        {Path(record["nativeArtifact"])}
        if "nativeArtifact" in record
        else set()
    )
    if "nativeBefore" not in record:
        return roots
    root = CACHE / "native"
    guard(root)
    if not root.exists():
        return roots
    before = set(record["nativeBefore"])
    descriptor = directory(CACHE / "native")
    try:
        roots.update(
            CACHE / "native" / entry.name
            for entry in entries(descriptor, counter)
            if entry.is_dir(follow_symlinks=False) and entry.name not in before
        )
    finally:
        os.close(descriptor)
    return roots


def input_sources(
    record: dict[str, Any], counter: list[int]
) -> Iterator[tuple[Path, Path]]:
    """Select immutable input evidence without scanning unrelated native caches.

    Yields:
        Selected base identity and selected/new native attempt evidence.
    """
    base = CACHE / "bases" / record["baseFingerprint"]
    guard(base)
    for name in ("prepared.json", "runtime.txt"):
        source = base / name
        if source.exists():
            yield source, Path("inputs/base") / name
    for root in sorted(native_sources(record, counter)):
        guard(root)
        for name in ("build.json", "build.log", "cargo.jsonl", "failure.json"):
            source = root / name
            if source.exists():
                yield source, Path("inputs/native") / root.name / name


def matches(path: Path, patterns: tuple[str, ...]) -> bool:
    """Return whether a complete relative path matches the allowlist."""
    return any(
        len(path.parts) == len(Path(pattern).parts) and path.match(pattern)
        for pattern in patterns
    )


def walk(descriptor: int, relative: Path, counter: list[int]) -> Iterator[Path]:
    """Traverse only allowlisted subtrees using directory-relative descriptors.

    Yields:
        Matching regular-file paths, never links or transformed OCR rasters.
    """
    for entry in entries(descriptor, counter):
        path = relative / entry.name
        if entry.is_dir(follow_symlinks=False):
            if matches(path, PREFIXES):
                child = os.open(
                    entry.name,
                    os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                    dir_fd=descriptor,
                )
                try:
                    yield from walk(child, path, counter)
                finally:
                    os.close(child)
        elif matches(path, PATTERNS) and not path.name.endswith(".ocr.png"):
            yield CACHE / path


def sources(path: Path) -> Iterator[tuple[Path, Path]]:
    """Select evidence from the proven cache and count every enumerated entry.

    Yields:
        Source files paired with artifact-relative destinations.

    Raises:
        RuntimeError: The fresh cache ownership no longer matches.
    """
    record = json.loads(path.read_text(encoding="utf-8"))
    yield path, Path("shared-result.json")
    for name in sorted(LOGS):
        source = path.parent / (name + ".log")
        if source.exists():
            yield source, Path("orchestration") / source.name
    guard(CACHE)
    if "cacheOwner" not in record or not CACHE.exists():
        return
    actual = CACHE.parent.stat()
    identity = {"device": actual.st_dev, "inode": actual.st_ino}
    if record["root"] != str(ROOT) or record["cacheOwner"] != identity:
        message = "Evidence cache ownership changed"
        raise RuntimeError(message)
    counter = [0]
    yield from input_sources(record, counter)
    descriptor = directory(CACHE)
    try:
        for source in walk(descriptor, Path(), counter):
            yield source, Path("desktop") / source.relative_to(CACHE)
    finally:
        os.close(descriptor)


def copy(source: Path, destination: Path, remaining: int) -> dict[str, Any]:
    """Retain complete JSON/PNG/TSV or an explicitly marked log tail.

    Returns:
        Retained-byte checksum and exact original/retained byte counts.

    Raises:
        RuntimeError: A changed file or evidence bound prevents capture.
    """
    guard(source)
    descriptor = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(descriptor, "rb") as incoming:
        metadata = os.fstat(incoming.fileno())
        if not stat.S_ISREG(metadata.st_mode):
            message = f"Non-regular evidence rejected: {source}"
            raise RuntimeError(message)
        size = metadata.st_size
        limit = 16 * MIB if source.suffix == ".png" else 4 * MIB
        truncated = source.suffix in {".log", ".jsonl"} and size > MIB
        retained = min(size, MIB) if truncated else size
        if retained > min(limit, remaining):
            message = f"Evidence bound exceeded: {source} ({size} bytes)"
            raise RuntimeError(message)
        if truncated:
            incoming.seek(-MIB, os.SEEK_END)
        content = incoming.read(retained + 1)
    if len(content) != retained:
        message = f"Evidence changed during collection: {source}"
        raise RuntimeError(message)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(content)
    return {
        "originalBytes": size,
        "retainedBytes": retained,
        "truncated": truncated,
        "sha256": build_native.digest(destination),
    }


def capture_files(
    path: Path, evidence: Path, inventory: list[dict[str, Any]]
) -> None:
    """Copy bounded files while preserving completed entries on any failure.

    Raises:
        RuntimeError: The source list exceeds the retained file limit.
    """
    total = 0
    with contextlib.closing(sources(path)) as selected:
        for index, (source, relative) in enumerate(selected):
            if index >= MAX_FILES:
                message = f"Evidence file count exceeded {MAX_FILES}"
                raise RuntimeError(message)
            item = copy(source, evidence / relative, 256 * MIB - total)
            inventory.append({"path": str(relative), **item})
            total += item["retainedBytes"]


def collect(path: Path, evidence: Path) -> None:
    """Stage bounded evidence and persist inventory even when enumeration fails.

    Raises:
        RuntimeError: Required evidence was rejected or exceeded a bound.
    """
    evidence.mkdir(parents=True, exist_ok=True)
    inventory, failures = [], []
    try:
        capture_files(path, evidence, inventory)
    except (OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
        failures.append(str(error))
    finally:
        vm_control.save(
            evidence / "inventory.json",
            {
                "files": inventory,
                "totalBytes": sum(item["retainedBytes"] for item in inventory),
                "errors": failures,
            },
        )
    if failures:
        message = "; ".join(failures)
        raise RuntimeError(message)
