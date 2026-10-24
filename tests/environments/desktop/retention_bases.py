"""Publish output ownership and retire only unused verified desktop bases."""  # ruff: ignore[implicit-namespace-package] - Standalone environment tooling.

import hashlib
import json
import os
import stat
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Catch bounded qemu-img inspection failures.
from collections.abc import Callable
from contextlib import ExitStack
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import retention_payload
import vm_control

OUTPUTS = {
    "base.qcow2",
    "vars.fd",
    "id_ed25519",
    "id_ed25519.pub",
    "known_hosts",
    "runtime.txt",
    "prepared.json",
}
MAX_SEED_BYTES = 65536


def publish(base: Path) -> None:
    """Publish integrity first and the prepared readiness marker last."""
    files = {}
    for name in sorted(OUTPUTS):
        path = base / (
            "prepared.pending.json" if name == "prepared.json" else name
        )
        with path.open("rb") as source:
            files[name] = {
                "sizeBytes": os.fstat(source.fileno()).st_size,
                "sha256": hashlib.file_digest(source, "sha256").hexdigest(),
            }
    vm_control.save(
        base / "integrity.json",
        {
            "version": 1,
            "fingerprint": base.name,
            "files": files,
            "createdAt": datetime.now(UTC).isoformat(),
        },
    )
    (base / "prepared.pending.json").replace(base / "prepared.json")


def _seed_directory(path: Path) -> None:
    # seed.build emits precisely these six small UTF-8 files, never disks.
    expected = {
        "user_configuration.json",
        "user_credentials.json",
        "authorized_keys",
        "user_full_name.txt",
        "user_email_address.txt",
        "user_encrypt_installation.txt",
    }
    with retention_payload.directory(path) as directory:
        if set(os.listdir(directory)) != expected:  # ruff: ignore[os-listdir] - Inspect the verified directory descriptor.
            message = f"Unknown CIDATA seed inventory: {path}"
            raise ValueError(message)
        for name in expected:
            descriptor = os.open(
                name,
                os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
                dir_fd=directory,
            )
            with os.fdopen(descriptor, "rb") as source:
                metadata = os.fstat(source.fileno())
                if (
                    not stat.S_ISREG(metadata.st_mode)
                    or metadata.st_size > MAX_SEED_BYTES
                ):
                    message = f"Unknown CIDATA seed file: {path / name}"
                    raise ValueError(message)
                text = source.read(MAX_SEED_BYTES + 1).decode("utf-8")
                after = os.fstat(source.fileno())
                if (after.st_size, after.st_mtime_ns, after.st_ctime_ns) != (
                    metadata.st_size,
                    metadata.st_mtime_ns,
                    metadata.st_ctime_ns,
                ):
                    message = (
                        f"CIDATA seed changed during inspection: {path / name}"
                    )
                    raise ValueError(message)
                if name.endswith(".json") and not isinstance(
                    json.loads(text), dict
                ):
                    message = f"Malformed CIDATA seed JSON: {path / name}"
                    raise ValueError(message)


def _disk_references(path: Path, owner: Path, cache: Path) -> set[str]:
    references = set()
    with retention_payload.directory(owner):
        if not stat.S_ISREG(path.lstat().st_mode):
            message = f"Unknown managed disk type: {path}"
            raise ValueError(message)
    chain = json.loads(
        vm_control.output(
            [
                "qemu-img",
                "info",
                "--force-share",
                "--backing-chain",
                "--output=json",
                path,
            ],
            timeout=15,
        )
    )
    if not isinstance(chain, list) or not chain:
        message = f"Unknown managed backing chain: {path}"
        raise ValueError(message)
    for layer in chain:
        backing = layer.get("full-backing-filename")
        if backing:
            target = Path(backing)
            if (
                target.parent.parent != cache / "bases"
                or target.name != "base.qcow2"
            ):
                message = f"Unmanaged backing chain: {target}"
                raise ValueError(message)
            references.add(target.parent.name)
    return references


def _managed_entry(path: Path, group: str) -> bool:
    metadata = path.lstat()
    if stat.S_ISLNK(metadata.st_mode):
        message = f"Symlink in managed storage inventory: {path}"
        raise ValueError(message)
    if (
        stat.S_ISDIR(metadata.st_mode)
        and path.name == "cidata"
        and group in {"preparations", "guests"}
    ):
        _seed_directory(path)
        return False
    allowed = {
        "scenarios",
        "deployments",
        "panel-deployments",
        "peer-cases",
    }
    if stat.S_ISDIR(metadata.st_mode) and path.name not in allowed:
        message = f"Unknown nested managed storage: {path}"
        raise ValueError(message)
    return True


def _references(read: Callable, children: Callable) -> set[str]:  # ruff: ignore[too-many-branches] - Census every supported managed writable-disk namespace.
    cache = vm_control.CACHE
    references = set()
    for path in (cache / "active.json", cache / "preparation.json"):
        if path.exists() or path.is_symlink():
            record, _ = read(path)
            references.add(record["fingerprint"])
    for group in ("labs", "guests", "preparations"):
        for owner in children(cache / group):
            for path in children(owner):
                if not _managed_entry(path, group):
                    continue
                if path.name == "state.json":
                    record, _ = read(path)
                    if record.get("directory") != str(owner):
                        message = f"Managed state owner differs: {path}"
                        raise ValueError(message)
                    references.add(record["fingerprint"])
                if path.suffix == ".qcow2":
                    references.update(_disk_references(path, owner, cache))
    return references


def _latest(read: Callable, children: Callable) -> str:
    records = []
    for base in children(vm_control.CACHE / "bases"):
        record, _ = read(base / "prepared.json")
        if record.get("fingerprint") != base.name:
            message = "Ambiguous base publication inventory"
            raise ValueError(message)
        records.append((
            datetime.fromisoformat(record["preparedAt"]),
            base.name,
        ))
    return max(records)[1] if records else ""


def sweep(  # ruff: ignore[too-many-arguments] - Reuse existing evidence and lifecycle-lock operations.
    report: dict[str, Any],
    hours: float,
    *,
    read: Callable,
    children: Callable,
    lock: Callable,
    save: Callable,
) -> None:
    """Retire only integrity-manifest bases; legacy publications stay intact."""
    for base in children(vm_control.CACHE / "bases"):
        try:
            _retire(base, hours, report, read, children, lock, save)
        except (
            OSError,
            ValueError,
            KeyError,
            TypeError,
            subprocess.SubprocessError,
        ) as error:
            report["items"].append({
                "path": str(base),
                "status": "protected",
                "category": "retained-base",
                "reason": str(error),
            })
        save()


def _prepared_for_retirement(
    base: Path, hours: float, read: Callable
) -> dict[str, Any]:
    prepared, _ = read(base / "prepared.json")
    identity = hashlib.sha256(
        json.dumps(prepared["inputs"], sort_keys=True).encode()
    ).hexdigest()
    if identity != base.name or prepared["fingerprint"] != base.name:
        message = "Base fingerprint differs from recorded preparation inputs"
        raise ValueError(message)
    age = (
        datetime.now(UTC) - datetime.fromisoformat(prepared["preparedAt"])
    ).total_seconds() / 3600
    if age < hours:
        message = "Base has not reached its retention age"
        raise ValueError(message)
    return prepared


def _retirement_outputs(
    base: Path, read: Callable, children: Callable
) -> tuple:
    try:
        manifest, checksum = read(base / "integrity.json")
    except FileNotFoundError as error:
        message = "Legacy base has no output-integrity manifest; retained"
        raise ValueError(message) from error
    if (
        manifest.get("version") != 1
        or manifest.get("fingerprint") != base.name
        or set(manifest["files"]) != OUTPUTS
    ):
        message = "Legacy or malformed base output provenance; retained"
        raise ValueError(message)
    if {path.name for path in children(base)} != OUTPUTS | {"integrity.json"}:
        message = "Unknown base inventory; retained"
        raise ValueError(message)
    expected = {
        **manifest["files"],
        "integrity.json": {
            "sha256": checksum,
            "sizeBytes": (base / "integrity.json").lstat().st_size,
        },
    }
    candidates = []
    # Hash large immutable outputs before taking any lifecycle locks.
    for name, metadata in expected.items():
        item = retention_payload.payload(base / name, metadata)
        if item["status"] != "eligible":
            message = f"Base output is not intact: {item}"
            raise ValueError(message)
        item.update(location="host", category="expired-base-output")
        candidates.append(item)
    return manifest, candidates


def _retirement_locks(
    children: Callable, lock: Callable, locks: ExitStack
) -> list:
    cache = vm_control.CACHE
    with retention_payload.directory(cache) as directory:
        descriptor = os.open(
            "bases.lock",
            os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
            0o600,
            dir_fd=directory,
        )
        os.close(descriptor)
    locks.enter_context(lock(cache / "prepare.lock"))
    locks.enter_context(lock(cache / "operation.lock"))
    locks.enter_context(lock(cache / "bases.lock"))
    owners = children(cache / "labs")
    for owner in owners:
        if not children(owner):
            # Never-started owner has no disk/reference. bases.lock prevents
            # its first acquisition until this retirement phase completes.
            continue
        locks.enter_context(lock(owner / "operation.lock"))
    return owners


def _revalidate_outputs(directory: int, candidates: list) -> None:
    for item in candidates:
        current = os.stat(
            Path(item["path"]).name,
            dir_fd=directory,
            follow_symlinks=False,
        )
        if (
            list(retention_payload._identity(current))  # ruff: ignore[private-member-access] - Revalidate the same shared helper's descriptor identity.
            != item["identity"]
        ):
            message = "Base output changed after hash verification"
            raise ValueError(message)


def _remove_outputs(directory: int, candidates: list, save: Callable) -> None:
    # Withdraw readiness first, before removing any prepared output.
    candidates.sort(key=lambda item: Path(item["path"]).name != "prepared.json")
    for item in candidates:
        item["status"] = "removal-pending"
        save()
        os.unlink(Path(item["path"]).name, dir_fd=directory)
        item["status"] = "removed"
        save()


def _retire(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Large-file verification precedes the short locked retirement phase.
    base: Path,
    hours: float,
    report: dict[str, Any],
    read: Callable,
    children: Callable,
    lock: Callable,
    save: Callable,
) -> None:
    prepared = _prepared_for_retirement(base, hours, read)
    manifest, candidates = _retirement_outputs(base, read, children)
    cache = vm_control.CACHE
    with ExitStack() as locks:
        owners = _retirement_locks(children, lock, locks)
        references = _references(read, children)
        if base.name in references or base.name == _latest(read, children):
            message = (
                "Managed storage/state or latest publication references base"
            )
            raise ValueError(message)
        if children(cache / "labs") != owners:
            message = (
                "Managed lab inventory changed during backing-chain census"
            )
            raise ValueError(message)
        # Hashing finished before the short lifecycle-lock phase.
        with retention_payload.directory(base) as directory:
            _revalidate_outputs(directory, candidates)
            report["items"].extend(candidates)
            report.setdefault("retiredBaseEvidence", {})[base.name] = {
                "prepared": prepared,
                "integrity": manifest,
                "runtime": (base / "runtime.txt").read_text(encoding="utf-8"),
            }
            save()
            if report["mode"] == "apply":
                _remove_outputs(directory, candidates, save)
                base.rmdir()
