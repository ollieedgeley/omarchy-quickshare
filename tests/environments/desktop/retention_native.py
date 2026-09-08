"""Reclaim old unpublished-to-labs native payloads, retaining build evidence."""  # ruff: ignore[implicit-namespace-package] - Imported by standalone retention tooling.

import json
import os
import re
import shlex
import stat
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Catch bounded pinned SSH failures.
import uuid
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import build_native
import deploy_native
import retention_payload
import vm_control


def _live_identity(state: dict[str, Any]) -> set[str]:
    if not vm_control.running(state):
        message = "Ambiguous stopped lab protects native payloads"
        raise RuntimeError(message)
    command = deploy_native.SESSION + shlex.join([
        "python3",
        "-c",
        deploy_native.GUEST,
        "identity",
    ])
    record = json.loads(
        vm_control.ssh(state, "bash -lc " + shlex.quote(command)).stdout
    )
    return {
        "sha256:" + value
        for component in record["components"].values()
        for key, value in component.items()
        if key in {"loadedSha256", "installedSha256"}
    }


def _references(read: Callable, children: Callable) -> set[str]:  # ruff: ignore[too-many-branches] - Each managed owner and record must be unambiguous before pruning.
    references = set()
    for pointer in children(vm_control.CACHE / "native"):
        if pointer.name in {
            "latest.json",
            "latest-cli.json",
            "latest-daemon.json",
        }:
            latest, _ = read(pointer)
            references.add(latest["artifact"])
    for owner in children(vm_control.CACHE / "labs"):
        state_path = owner / "state.json"
        # A never-started owner containing only its lock cannot own a deploy.
        if {path.name for path in children(owner)} <= {"operation.lock"}:
            continue
        state, _ = read(state_path)
        if state.get("directory") != str(owner) or state.get(
            "stateFile"
        ) != str(state_path):
            message = f"Ambiguous managed lab ownership: {owner}"
            raise ValueError(message)
        ambiguous = False
        for attempt in children(owner / "deployments"):
            record, _ = read(attempt / "deployment.json")
            if (
                record.get("deploymentId") != attempt.name
                or record.get("lab") != owner.name
            ):
                message = f"Ambiguous deployment identity: {attempt}"
                raise ValueError(message)
            artifact = record.get("artifact")
            if artifact:
                references.add(artifact)
            else:
                ambiguous = True
        if ambiguous:
            references.update(_live_identity(state))
    return references


def sweep(  # ruff: ignore[too-many-arguments] - Explicit existing record and lock operations, no duplicate ownership implementation.
    report: dict[str, Any],
    hours: float,
    *,
    read: Callable,
    children: Callable,
    lock: Callable,
    save: Callable,
) -> None:
    """Append exact native eligibility and removals under publication locks."""
    root = vm_control.CACHE / "native"
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Any ambiguous managed reference protects all native payloads.
        with retention_payload.directory(root) as directory:
            descriptor = os.open(
                "artifacts.lock",
                os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
                0o600,
                dir_fd=directory,
            )
            os.close(descriptor)
        with lock(root / "build.lock"), lock(root / "artifacts.lock"):
            references = _references(read, children)
            report["nativeProtectedReferences"] = sorted(references)
            for artifact in children(root):
                if artifact.name in {
                    "build.lock",
                    "artifacts.lock",
                    "cargo-target",
                    "latest.json",
                    "latest-cli.json",
                    "latest-daemon.json",
                }:
                    continue
                try:
                    _artifact(
                        artifact,
                        references,
                        hours,
                        report,
                        read,
                        children,
                        save,
                    )
                except (OSError, ValueError, KeyError, TypeError) as error:
                    report["items"].append({
                        "path": str(artifact),
                        "category": "retained-native",
                        "status": "protected",
                        "reason": str(error),
                    })
    except (
        OSError,
        ValueError,
        KeyError,
        TypeError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        report["items"].append({
            "path": str(root),
            "category": "active-or-unknown-native",
            "status": "protected",
            "reason": str(error),
        })
    save()


def _artifact(  # ruff: ignore[too-many-arguments, too-many-positional-arguments, too-many-branches, complex-structure, too-many-statements] - Preserve one artifact's identity across classification and exact unlink.
    artifact: Path,
    references: set[str],
    hours: float,
    report: dict[str, Any],
    read: Callable,
    children: Callable,
    save: Callable,
) -> None:
    if str(artifact) in references:
        message = "Latest or retained deployment reference; payload kept"
        raise ValueError(message)
    record, checksum = read(artifact / "build.json")
    if record.get("schemaVersion") != build_native.SCHEMA_VERSION:
        message = "Unknown historical native schema protects its payloads"
        raise ValueError(message)
    records = record.get("components", {})
    if (
        not isinstance(records, dict)
        or not records
        or not records.keys() <= build_native.COMPONENTS.keys()
    ):
        message = "Unknown native component inventory protects its payloads"
        raise ValueError(message)
    payloads = {}
    for role, component in records.items():
        name = build_native.COMPONENTS[role]["file"]
        if component.get("file") != name:
            message = "Unknown native component filename"
            raise ValueError(message)
        payloads[name] = component["sha256"]
    if "daemon" in records:
        payloads[build_native.UNIT.name] = record["unitSha256"]
    if any("sha256:" + digest in references for digest in payloads.values()):
        message = "Installed or loaded guest native executable retained"
        raise ValueError(message)
    if (
        str(uuid.UUID(artifact.name)) != artifact.name
        or record.get("buildId") != artifact.name
        or record.get("status") != "built"
    ):
        message = "Unknown, unfinished or failed native build retained"
        raise ValueError(message)
    names = {path.name for path in children(artifact)}
    if not names <= {*payloads, "build.json", "build.log", "cargo.jsonl"}:
        message = (
            "Unknown inventory or failure evidence protects native attempt"
        )
        raise ValueError(message)
    with retention_payload.directory(artifact) as directory:
        for name in names:
            metadata = os.stat(name, dir_fd=directory, follow_symlinks=False)
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                message = "Native inventory contains a symlink or special file"
                raise ValueError(message)
    finished = datetime.fromisoformat(record["finishedAt"])
    age = (datetime.now(UTC) - finished).total_seconds() / 3600
    if finished < datetime.fromisoformat(record["startedAt"]) or age < hours:
        message = "Native build has not reached its retention age"
        raise ValueError(message)
    for name, digest in payloads.items():
        expected = {"sha256": digest}
        item = retention_payload.payload(artifact / name, expected)
        item.update(
            location="host",
            category="expired-native-payload",
            buildRecordSha256=checksum,
            retentionReason="Expired native payload; build evidence retained",
        )
        report["items"].append(item)
        save()
        if report["mode"] == "apply" and item["status"] == "eligible":
            if read(artifact / "build.json")[1] != checksum:
                message = "Native build record changed during cleanup"
                raise ValueError(message)
            item["status"] = "removal-pending"
            save()
            item.update(
                retention_payload.payload(artifact / name, expected, apply=True)
            )
            save()


def _cache_files(root: Path) -> dict[str, os.stat_result]:
    files = {}
    pending = [root]
    while pending:
        directory = pending.pop()
        with retention_payload.directory(directory) as descriptor:
            for name in os.listdir(descriptor):  # ruff: ignore[os-listdir] - Walk verified descriptors without following symlinks.
                path = directory / name
                metadata = os.stat(
                    name, dir_fd=descriptor, follow_symlinks=False
                )
                if stat.S_ISDIR(metadata.st_mode):
                    pending.append(path)
                elif stat.S_ISREG(metadata.st_mode):
                    files[str(path)] = metadata
                else:
                    message = f"Unknown cache symlink or special file: {path}"
                    raise ValueError(message)
    return files


def prune_cache(  # ruff: ignore[too-many-arguments, too-many-branches] - Explicit selected cache policy and existing ownership operations.
    report: dict[str, Any],
    hours: float,
    *,
    profile: str,
    read: Callable,
    children: Callable,
    lock: Callable,
    save: Callable,
) -> None:
    """Clean an explicitly selected old compiler cache.

    Raises:
        ValueError: Unsupported or recent caches produce protected outcomes.
    """
    root = vm_control.CACHE / "native/cargo-target"
    item = {
        "path": str(root),
        "category": "selected-native-compiler-cache",
        "status": "protected",
        "profile": profile,
        "location": "host",
        "files": 0,
    }
    report["items"].append(item)
    try:  # ruff: ignore[too-many-statements-in-try-clause] - The compiler lock spans provenance, age check and Cargo's supported cleaner.
        with lock(root.parent / "build.lock"):
            records = []
            targets = set()
            for artifact in children(root.parent):
                if not re.fullmatch(r"[0-9a-f-]{36}", artifact.name):
                    continue
                record, checksum = read(artifact / "build.json")
                if record.get("cache", {}).get("compilerDirectory") == str(
                    root
                ):
                    target = _cache_target(record, root, profile)
                    if target is None:
                        continue
                    targets.add(target)
                    records.append({
                        "path": str(artifact / "build.json"),
                        "sha256": checksum,
                        "target": target,
                    })
            if not records:
                message = (
                    "No native build metadata identifies this compiler cache"
                )
                raise ValueError(message)  # ruff: ignore[raise-within-try] - Retain unsupported caches.
            before = _cache_files(root)
            newest = max(
                (value.st_mtime for value in before.values()),
                default=root.stat().st_mtime,
            )
            age = (datetime.now(UTC).timestamp() - newest) / 3600
            if age < hours:
                message = "Compiler cache is newer than its retention age"
                raise ValueError(message)  # ruff: ignore[raise-within-try] - Preserve warm inputs unless explicit age permits pruning.
            # Cargo's whole-profile clean ignores --target. Select each
            # recorded target root directly; constrain build-dir too.
            roots = [root, *(root / target for target in sorted(targets))]
            commands = [
                [
                    "cargo",
                    "clean",
                    "--manifest-path",
                    str(vm_control.ROOT / "Cargo.toml"),
                    "--target-dir",
                    str(selected),
                    "--profile",
                    profile,
                    "--config",
                    "build.build-dir=" + json.dumps(str(selected)),
                ]
                for selected in roots
            ]
            item.update(
                status="eligible",
                reason="Explicit owned cache profile prune",
                ageHours=age,
                ownershipRecords=records,
                commands=commands,
                targets=sorted(targets),
                profileDirectories=[
                    str(selected / ("debug" if profile == "dev" else "release"))
                    for selected in roots
                ],
                beforeFiles=len(before),
                beforeLogicalBytes=sum(
                    value.st_size for value in before.values()
                ),
                beforeAllocatedFileBytes=sum(
                    value.st_blocks * 512 for value in before.values()
                ),
            )
            save()
            if report["mode"] == "apply":
                item["status"] = "removal-pending"
                save()
                item.update(_clean_cache(commands, root, before))
    except (
        OSError,
        ValueError,
        KeyError,
        TypeError,
        subprocess.SubprocessError,
    ) as error:
        item.update(status="protected", reason=str(error))
    save()


def _cache_target(  # ruff: ignore[too-many-branches] - Validate each recorded compiler cache boundary.
    record: dict[str, Any], root: Path, profile: str
) -> str | None:
    if record.get("schemaVersion") != build_native.SCHEMA_VERSION:
        message = "Unknown historical native schema protects its compiler cache"
        raise ValueError(message)
    command = record["command"]
    if not isinstance(command, list) or not all(
        isinstance(arg, str) for arg in command
    ):
        message = "Recorded compiler command is not a string argument list"
        raise TypeError(message)
    values = {}
    for flag in ("--target-dir", "--profile", "--target"):
        if command.count(flag) != 1 or command.index(flag) + 1 >= len(command):
            message = f"Ambiguous compiler cache command option: {flag}"
            raise ValueError(message)
        values[flag] = command[command.index(flag) + 1]
    if values["--target-dir"] != str(root):
        message = (
            "Recorded command target directory differs from cache ownership"
        )
        raise ValueError(message)
    if values["--profile"] != profile:
        return None
    target = values["--target"]
    if re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]+", target) is None:
        message = "Unsupported compiler cache target identity"
        raise ValueError(message)
    return target


def _clean_cache(
    commands: list[list[str]], root: Path, before: dict[str, os.stat_result]
) -> dict[str, Any]:
    outcome = {"executions": []}
    for command in commands:
        try:
            result = vm_control.run(
                command, capture_output=True, text=True, timeout=120
            )
            outcome["executions"].append({
                "command": command,
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exitCode": result.returncode,
            })
        except (OSError, subprocess.SubprocessError) as error:
            outcome["error"] = str(error)
            break
    after = _cache_files(root) if root.exists() else {}
    removed_paths = [path for path in before if path not in after]
    removed = [before[path] for path in removed_paths]
    profile_directories = [
        Path(command[command.index("--target-dir") + 1])
        / (
            "debug"
            if command[command.index("--profile") + 1] == "dev"
            else "release"
        )
        for command in commands
    ]
    remaining = [str(path) for path in profile_directories if path.exists()]
    if remaining and "error" not in outcome:
        outcome["error"] = "Cargo left selected profile directories in place"
    outcome.update(
        status=(
            "no-op"
            if not removed
            else "partial-removal"
            if "error" in outcome
            else "removed"
        ),
        files=len(removed),
        logicalBytes=sum(value.st_size for value in removed),
        allocatedFileBytes=sum(value.st_blocks * 512 for value in removed),
        removedPaths=removed_paths,
        profileDirectoriesRemaining=remaining,
        fullyPruned=not remaining and "error" not in outcome,
    )
    return outcome
