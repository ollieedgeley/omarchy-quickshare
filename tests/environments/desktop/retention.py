#!/usr/bin/env python3
"""Inspect/dry-run/apply retention for DESKTOP_LAB, default dry-run.

Passed synthetic payloads expire after 168 hours; unreferenced native
executables and units after 720 hours. Age options accept 0..87600.
Results, failures, reset records, logs, downloads and preparations are kept.
Verified unreferenced bases expire after 720 hours; legacy bases stay intact.
--prune-native-cache explicitly selects Cargo's cleaner for an old dev/release
profile. No guest shutdown or service reset occurs.
Every invocation retains a separate report. Guest blocks do not measure
host qcow2 reclamation. Virtual disk capacity is never counted.
"""

import argparse
import fcntl
import hashlib
import json
import math
import os
import shlex
import stat
import time
import uuid
from collections.abc import Callable, Iterator
from contextlib import contextmanager
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import deploy_native
import lab_lifecycle
import retention_bases
import retention_native
import retention_payload
import scenario_state
import vm_control

MAX_AGE_HOURS = 87600


def _now() -> str:
    return datetime.now(UTC).isoformat()


def _age(value: str) -> float:
    hours = float(value)
    if not math.isfinite(hours) or not 0 <= hours <= MAX_AGE_HOURS:
        message = "Age must be finite and between 0 and 87600 hours"
        raise argparse.ArgumentTypeError(message)
    return hours


def _read(path: Path) -> tuple[dict[str, Any], str]:
    with retention_payload.directory(path.parent) as parent:
        descriptor = os.open(
            path.name,
            os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK,
            dir_fd=parent,
        )
        with os.fdopen(descriptor, "rb") as source:
            if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
                message = f"Not a regular evidence file: {path}"
                raise ValueError(message)
            raw = source.read(8 * 1024 * 1024 + 1)
    value = json.loads(raw)
    if not isinstance(value, dict):
        message = f"Not a dictionary record: {path}"
        raise TypeError(message)
    return value, hashlib.sha256(raw).hexdigest()


def _children(path: Path) -> list[Path]:
    try:
        with retention_payload.directory(path) as descriptor:
            return [path / name for name in sorted(os.listdir(descriptor))]  # ruff: ignore[os-listdir] - List the verified directory descriptor, not a raced pathname.
    except FileNotFoundError:
        return []


@contextmanager
def _lock(path: Path) -> Iterator[None]:
    # Existing inodes only. A missing or held lock protects the entire scope.
    with retention_payload.directory(path.parent) as parent:
        descriptor = os.open(
            path.name, os.O_RDWR | os.O_NOFOLLOW, dir_fd=parent
        )
        with os.fdopen(descriptor, "r+") as lock:
            if not stat.S_ISREG(os.fstat(lock.fileno()).st_mode):
                message = f"Not a regular operation lock: {path}"
                raise ValueError(message)
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            yield


def _kept(
    path: Path, category: str, reason: str, **details: object
) -> dict[str, Any]:
    return {
        "path": str(path),
        "category": category,
        "status": "protected",
        "reason": reason,
        **details,
    }


def _inventory(report: dict[str, Any]) -> None:
    cache = vm_control.CACHE
    items = report["items"]
    references = set()
    for path in [cache / "active.json", cache / "preparation.json"] + [
        owner / "state.json" for owner in _children(cache / "labs")
    ]:
        try:
            record, checksum = _read(path)
            references.add(record.get("fingerprint"))
            items.append(
                _kept(
                    path,
                    "active-or-retained-resource",
                    "Resource state and writable storage never expire",
                    recordSha256=checksum,
                    running=vm_control.running(record),
                )
            )
        except (OSError, ValueError, KeyError) as error:
            items.append(_kept(path, "unknown", str(error)))
    for path, reason in [
        (
            cache / "native",
            "Compiler targets and deployed artifacts kept; others age out",
        ),
        (
            cache / "downloads",
            "Verified ISO and partial downloads kept; no input eviction",
        ),
        (
            cache / "tools",
            "Pinned extracted tools and packages kept indefinitely",
        ),
        (
            cache / "preparations",
            "Preparation evidence and ambiguous partial disks kept",
        ),
        (
            cache / "guests",
            "Historical writable guests kept; no inferred ownership",
        ),
        (
            cache.parent / "nearby-linux",
            "Provisioned images, inputs and evidence kept; lock unchanged",
        ),
    ]:
        items.append(_kept(path, "reusable-or-retained-input", reason))


def _live(state: dict[str, Any]) -> dict[str, Any]:
    if not vm_control.running(state):
        message = "Guest unavailable; retain payloads until live inspection"
        raise RuntimeError(message)
    command = (
        deploy_native.SESSION
        + 'timeout 10 "$HOME/.local/bin/omarchy-quickshare" status --json'
    )
    envelope = json.loads(
        vm_control.ssh(state, "bash -lc " + shlex.quote(command)).stdout
    )
    response = envelope["response"]
    if response["type"] != "snapshot":
        message = "Application did not return a public snapshot"
        raise RuntimeError(message)
    active = response["snapshot"].get("active_share")
    if active and active["phase"] not in scenario_state.TERMINAL:
        message = (
            "Active application transfer protects all payloads in this lab"
        )
        raise RuntimeError(message)
    peer, _ = scenario_state.peer_state(Path(state["directory"]))
    connections = scenario_state._connections(peer)  # ruff: ignore[private-member-access] - Read-only owned receiver connection probe, without reset or evidence writes.
    if connections["guestConnections"] or len(connections["receiverPids"]) != 1:
        message = (
            "Reference receiver is busy or its process identity is ambiguous"
        )
        raise RuntimeError(message)
    return {"activeShare": active, "peer": peer, "connections": connections}


def _candidate(  # ruff: ignore[too-many-arguments, too-many-positional-arguments, too-many-branches] - Identity, age and live ownership are separate mandatory checks.
    owner: Path,
    attempt: Path,
    state: dict[str, Any],
    hours: float,
    live: dict[str, Any],
) -> tuple[dict[str, Any], str]:
    record, checksum = _read(attempt / "result.json")
    expected_identity = {
        "attemptId": attempt.name,
        "scenario": "file-to-reference-peer",
        "lab": owner.name,
        "guestUuid": state["uuid"],
        "status": "passed",
    }
    if str(uuid.UUID(attempt.name)) != attempt.name or any(
        record.get(key) != value for key, value in expected_identity.items()
    ):
        message = (
            "Failed, unfinished, historical guest or ambiguous attempt retained"
        )
        raise ValueError(message)
    finished = datetime.fromisoformat(record["finishedAt"])
    started = datetime.fromisoformat(record["startedAt"])
    age = (datetime.now(UTC) - finished).total_seconds() / 3600
    if finished < started or age < hours:
        message = (
            "Completion age is invalid or below the configured retention age"
        )
        raise ValueError(message)
    submitted, received = record["submitted"], record["received"]
    name = f"quickshare-{attempt.name}.bin"
    synthetic = (
        f"Synthetic retained-lab transfer {attempt.name}\n".encode()
        + bytes(range(256)) * 256
    )
    expected = {
        "name": name,
        "sizeBytes": len(synthetic),
        "sha256": hashlib.sha256(synthetic).hexdigest(),
    }
    for metadata in (submitted, received):
        if any(metadata.get(key) != value for key, value in expected.items()):
            message = "Payload differs from the attempt's synthetic bytes"
            raise ValueError(message)
    if (
        submitted["sourcePath"] != str(attempt / name)
        or received["path"] != str(owner / "peer-cases/received" / name)
        or submitted["guestPath"]
        != f".cache/omarchy-quickshare-scenarios/{attempt.name}/{name}"
    ):
        message = "Recorded payload paths differ from exact owned locations"
        raise ValueError(message)
    active = live["activeShare"]
    if active and (
        active["id_string"] == record.get("shareId")
        or active.get("attachment", {}).get("name") == name
    ):
        message = "Current share source retained for owned reset"
        raise ValueError(message)
    reset, _ = _read(attempt / "reset.json")
    if (
        reset.get("status") != "ready"
        or record.get("publicOutcome", {}).get("phase") != "completed"
    ):
        message = "Completion or independent reset evidence is ambiguous"
        raise ValueError(message)
    return record, checksum


def _guest(
    state: dict[str, Any], record: dict[str, Any], *, apply: bool
) -> dict[str, Any]:
    request = {
        "path": record["submitted"]["guestPath"],
        "expected": record["submitted"],
        "apply": apply,
    }
    source = Path(retention_payload.__file__).read_text(encoding="utf-8")
    command = shlex.join(["python3", "-c", source, json.dumps(request)])
    result = vm_control.ssh(state, "bash -lc " + shlex.quote(command))
    item = json.loads(result.stdout)
    item.update(location="guest", guestUuid=state["uuid"])
    return item


def _scenarios(  # ruff: ignore[too-many-arguments, too-many-positional-arguments, too-many-branches] - One selected-owner lock spans eligibility and removal.
    owner: Path, report: dict[str, Any], hours: float, save: Callable[[], None]
) -> None:
    with _lock(owner / "operation.lock"):
        # Check no-follow evidence before using the existing owner validator.
        _read(owner / "state.json")
        state = lab_lifecycle.active(owner)
        if not state:
            message = "Selected lab has no owned state"
            raise RuntimeError(message)
        live = _live(state)
        report["live"] = live
        for attempt in _children(owner / "scenarios"):
            try:
                record, checksum = _candidate(
                    owner, attempt, state, hours, live
                )
            except (OSError, ValueError, KeyError, TypeError) as error:
                report["items"].append(
                    _kept(attempt, "retained-scenario-evidence", str(error))
                )
                continue
            report["items"].append(
                _kept(
                    attempt / "result.json",
                    "retained-result",
                    "Original result and reset bytes are never rewritten",
                    recordSha256=checksum,
                )
            )
            expected = record["submitted"]
            candidates = [
                retention_payload.payload(Path(path), expected)
                for path in (expected["sourcePath"], record["received"]["path"])
            ]
            candidates.append(_guest(state, record, apply=False))
            for item in candidates:
                item.setdefault("location", "host")
                item.update(
                    category="completed-synthetic-payload",
                    resultPath=str(attempt / "result.json"),
                    resultSha256=checksum,
                )
                report["items"].append(item)
            save()
            if report["mode"] != "apply" or any(
                item["status"] not in {"eligible", "absent"}
                for item in candidates
            ):
                continue
            # Recheck live work and immutable result before unlinking.
            _candidate(owner, attempt, state, hours, _live(state))
            if _read(attempt / "result.json")[1] != checksum:
                message = "Result changed during cleanup; refusing removal"
                raise RuntimeError(message)
            for item in candidates:
                if item["status"] == "absent":
                    continue
                item["status"] = "removal-pending"
                save()
                outcome = (
                    _guest(state, record, apply=True)
                    if item["location"] == "guest"
                    else retention_payload.payload(
                        Path(item["path"]), expected, apply=True
                    )
                )
                item.update(outcome)
                save()
            if _read(attempt / "result.json")[1] != checksum:
                message = "Immutable result changed outside cleanup"
                raise RuntimeError(message)


def _totals(items: list[dict[str, Any]]) -> dict[str, Any]:
    result = {}
    for location in ("host", "guest"):
        result[location] = {}
        for status in ("eligible", "removed", "partial-removal"):
            selected = [
                item
                for item in items
                if item.get("location") == location and item["status"] == status
            ]
            result[location][status] = {
                "files": sum(item.get("files", 1) for item in selected),
                "logicalBytes": sum(
                    item.get("logicalBytes", 0) for item in selected
                ),
                "allocatedFileBytes": sum(
                    item.get("allocatedFileBytes", 0) for item in selected
                ),
            }
    return result


def main() -> None:  # ruff: ignore[too-many-statements] - One CLI boundary records all requested independent cleanup classes.
    """Run retention and retain evidence, including interrupted runs.

    Raises:
        KeyboardInterrupt: Cleanup was interrupted after retaining its report.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "mode",
        choices=("inspect", "dry-run", "apply"),
        nargs="?",
        default="dry-run",
    )
    parser.add_argument("--scenario-age-hours", type=_age, default=168.0)
    parser.add_argument("--base-age-hours", type=_age, default=720.0)
    parser.add_argument("--native-age-hours", type=_age, default=720.0)
    parser.add_argument("--prune-native-cache", action="store_true")
    parser.add_argument(
        "--native-cache-profile", choices=("dev", "release"), default="dev"
    )
    parser.add_argument("--native-cache-age-hours", type=_age, default=720.0)
    args = parser.parse_args()
    os.umask(0o077)
    owner = lab_lifecycle.directory()
    root = vm_control.CACHE / "cleanups"
    with retention_payload.directory(root.parent):
        root.mkdir(mode=0o700, exist_ok=True)
    with retention_payload.directory(root):
        run = root / str(uuid.uuid4())
        run.mkdir(mode=0o700)
    destination = run / "cleanup.json"
    started = time.monotonic()
    report = {
        "cleanupId": run.name,
        "mode": args.mode,
        "lab": owner.name,
        "startedAt": _now(),
        "status": "running",
        "items": [],
        "policy": {
            "scenarioAgeHours": args.scenario_age_hours,
            "baseAgeHours": args.base_age_hours,
            "nativeAgeHours": args.native_age_hours,
            "pruneNativeCache": args.prune_native_cache,
            "nativeCacheProfile": args.native_cache_profile,
            "nativeCacheAgeHours": args.native_cache_age_hours,
            "evidence": "keep indefinitely",
            "reusableInputs": "downloads kept; compiler prune requires opt-in",
            "scope": "selected scenarios, native payloads and verified bases",
            "guestBlocksAreHostReclamation": False,
        },
    }

    def save() -> None:
        report["totals"] = _totals(report["items"])
        report["durationSeconds"] = time.monotonic() - started
        vm_control.save(destination, report)

    save()
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Preserve one report across independent cleanup classes.
        _inventory(report)
        retention_native.sweep(
            report,
            args.native_age_hours,
            read=_read,
            children=_children,
            lock=_lock,
            save=save,
        )
        retention_bases.sweep(
            report,
            args.base_age_hours,
            read=_read,
            children=_children,
            lock=_lock,
            save=save,
        )
        if args.prune_native_cache:
            retention_native.prune_cache(
                report,
                args.native_cache_age_hours,
                profile=args.native_cache_profile,
                read=_read,
                children=_children,
                lock=_lock,
                save=save,
            )
        try:
            _scenarios(owner, report, args.scenario_age_hours, save)
            report["scenarioStatus"] = "complete"
        except Exception as error:  # ruff: ignore[blind-except] - A protected scenario scope must not fail independent completed cleanup classes.
            report["scenarioStatus"] = "protected"
            report["items"].append(
                _kept(owner, "active-or-unknown-scenario-scope", str(error))
            )
        report["status"] = "complete"
    except (Exception, KeyboardInterrupt) as error:
        report.update(
            status="protected", error=f"{type(error).__name__}: {error}"
        )
        report["items"].append(
            _kept(owner, "active-or-unknown-scenario-scope", str(error))
        )
        raise
    finally:
        report["finishedAt"] = _now()
        save()
        print(json.dumps({"report": str(destination), **report}, indent=2))


if __name__ == "__main__":
    main()
