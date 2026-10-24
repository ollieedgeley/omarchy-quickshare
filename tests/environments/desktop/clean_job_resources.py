"""Preserve clean-job evidence and reclaim only its proven lab resources."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import fcntl
import json
import os
import shutil
import sys
import uuid
from pathlib import Path
from typing import Any

import candidate
import deploy_native
import deploy_panel
import lab_lifecycle
import reference_peer
import vm_control


def owner_for(job: dict[str, Any]) -> Path:
    """Derive the disposable owner instead of trusting an arbitrary path.

    Returns:
        The unique lab directory reserved by the job identifier.

    Raises:
        RuntimeError: The record is not a matching clean-job identity.
    """
    identifier = str(uuid.UUID(job["jobId"]))
    owner = (
        vm_control.CACHE / "labs" / ("job-" + identifier.replace("-", "")[:27])
    )
    attempt = vm_control.CACHE / "clean-installs" / identifier
    if (
        job.get("schemaVersion") != 1
        or job.get("kind") != "disposable-clean-job"
        or job.get("ownerDirectory") != str(owner)
        or job.get("installationDirectory") != str(attempt)
    ):
        message = "Clean-job identity/path mismatch; resources retained"
        raise RuntimeError(message)
    return owner


def proven_owner(job: dict[str, Any]) -> Path | None:
    """Check the installation's exclusive directory creation provenance.

    Returns:
        An owned directory, or None when installation created no resources.

    Raises:
        RuntimeError: An existing directory has no matching creation evidence.
    """
    owner = owner_for(job)
    if not owner.exists() and not owner.is_symlink():
        return None
    installation = vm_control.active(
        Path(job["installationDirectory"]) / "installation.json"
    )
    identity = owner.lstat()
    expected = {
        "attemptId": job["jobId"],
        "ownerDirectory": str(owner),
        "baseFingerprint": job["baseFingerprint"],
        "createdOwner": True,
        "ownerIdentity": {"device": identity.st_dev, "inode": identity.st_ino},
    }
    if (
        owner.is_symlink()
        or not owner.is_dir()
        or not installation
        or any(
            installation.get(key) != value for key, value in expected.items()
        )
    ):
        message = "Unproven fresh lab ownership; resources retained"
        raise RuntimeError(message)
    return owner


def preserve(owner: Path, evidence: Path, report: dict[str, Any]) -> None:
    """Snapshot lifecycle records before stop/remove rewrites them.

    Raises:
        RuntimeError: An evidence path is a symlink rather than an owned file.
    """
    report["preservedPaths"] = []
    for name in ("state.json", "peer.json", "qemu.pid"):
        source = owner / name
        if source.is_symlink():
            message = f"Symlink in owned lifecycle evidence: {source}"
            raise RuntimeError(message)
        if source.exists():
            destination = evidence / name
            shutil.copyfile(source, destination)
            report["preservedPaths"].append({
                "original": str(source),
                "snapshot": str(destination),
            })


def capture(owner: Path, evidence: Path, report: dict[str, Any]) -> None:
    """Gather final diagnostics without rewriting earlier evidence."""
    state = lab_lifecycle.active(owner)
    if state and vm_control.running(state):
        for journal in (deploy_native._journal, deploy_panel._journal):  # ruff: ignore[private-member-access] - Reuse existing bounded diagnostic capture.
            try:
                journal(state, evidence)
            except Exception as error:  # ruff: ignore[blind-except] - Capture failure must not prevent owned cleanup.
                report["errors"].append(
                    f"journal: {type(error).__name__}: {error}"
                )
        try:
            vm_control.qmp(
                state, "screendump", {"filename": str(evidence / "desktop.ppm")}
            )
        except Exception as error:  # ruff: ignore[blind-except] - Preserve capture failure alongside cleanup.
            report["errors"].append(
                f"screenshot: {type(error).__name__}: {error}"
            )
    if (owner / "peer.json").exists():
        try:
            record = reference_peer._retained(owner)  # ruff: ignore[private-member-access] - Existing peer ownership contract.
            reference_peer._owned(record)  # ruff: ignore[private-member-access] - Verify labels before reading private logs.
            logs = reference_peer._compose(record, ["logs", "--no-color"])  # ruff: ignore[private-member-access] - Existing bounded compose transport.
            (evidence / "peer.log").write_text(logs, encoding="utf-8")
        except Exception as error:  # ruff: ignore[blind-except] - Retain failure and continue independent owned cleanup.
            report["errors"].append(
                f"peer logs: {type(error).__name__}: {error}"
            )


def quiescent(owner: Path) -> None:
    """Refuse unlinking storage still named by any live QEMU process.

    Raises:
        RuntimeError: A surviving process references this lab's writable disk.
    """
    disk = ("file=" + str(owner / "desktop.qcow2")).encode()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            arguments = (entry / "cmdline").read_bytes().split(b"\0")
        except FileNotFoundError:
            continue
        if (
            arguments
            and b"qemu-system-" in arguments[0]
            and any(disk in argument.split(b",") for argument in arguments)
        ):
            message = f"Live QEMU {entry.name} still references owned storage"
            raise RuntimeError(message)


def reclaim(owner: Path, report: dict[str, Any]) -> None:  # ruff: ignore[too-many-branches] - Keep ordered ownership checks and exact resource removal together.
    """Stop the exact VM and peer before unlinking their writable storage.

    Raises:
        RuntimeError: A PID or resource is not proven to belong to this lab.
    """
    state = lab_lifecycle.active(owner)
    pidfile = owner / "qemu.pid"
    if pidfile.exists() and not vm_control.running(state):
        pid = int(pidfile.read_text(encoding="utf-8"))
        try:
            command = Path(f"/proc/{pid}/cmdline").read_bytes()
        except FileNotFoundError:
            command = b""
        if command:
            message = (
                "Recorded PID is live but does not match the owned VM; retained"
            )
            raise RuntimeError(message)
    vm_control.stop(state)
    if vm_control.running(state):
        message = "Owned guest remains live; writable storage retained"
        raise RuntimeError(message)
    quiescent(owner)
    if (owner / "peer.json").exists():
        reference_peer._stop(owner, remove=True)  # ruff: ignore[private-member-access] - Reuse label/project-checked peer removal after VM stop.
    # Keep logs, results, screenshots and received evidence at their original
    # paths. The only storage removed is this fresh owner's writable VM pair.
    for name in ("desktop.qcow2", "vars.fd"):
        path = owner / name
        if path.is_symlink() or (path.exists() and not path.is_file()):
            message = f"Unproven writable resource retained: {path}"
            raise RuntimeError(message)
        if path.exists():
            path.unlink()
            report["removedPaths"].append(str(path))


def supervision(path: Path, job: dict[str, Any]) -> None:
    """Require the wrapper's inherited lock and recorded cleanup session.

    Raises:
        RuntimeError: Cleanup was not launched by the locked job supervisor.
    """
    expected = (path.parent / "lifetime.lock").stat()
    actual = os.fstat(job["lockFd"])
    stage = job["stages"][-1]
    if (
        (actual.st_dev, actual.st_ino) != (expected.st_dev, expected.st_ino)
        or stage["name"] != "cleanup"
        or stage["status"] != "running"
        or os.getsid(0) != stage["worker"]["pid"]
    ):
        message = "Cleanup requires its recorded locked supervisor"
        raise RuntimeError(message)
    fcntl.flock(job["lockFd"], fcntl.LOCK_EX | fcntl.LOCK_NB)


def cleanup(path: Path) -> None:
    """Use the same locked cleanup for ordinary completion and recovery.

    Raises:
        RuntimeError: Evidence capture or owned reclamation could not finish.
    """
    job = json.loads(path.read_text(encoding="utf-8"))
    supervision(path, job)
    evidence = path.parent / "cleanup-evidence" / str(uuid.uuid4())
    evidence.mkdir(parents=True)
    report: dict[str, Any] = {
        "startedAt": candidate.now(),
        "status": "failed",
        "errors": [],
        "removedPaths": [],
        "evidenceDirectory": str(evidence),
    }
    vm_control.save(evidence / "cleanup.json", report)
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Preserve partial cleanup evidence at one error boundary.
        owner = proven_owner(job)
        if owner is not None:
            descriptor = os.open(
                owner / "operation.lock",
                os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
                0o600,
            )
            with os.fdopen(descriptor, "a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                # Recheck provenance after acquiring the actual lab lock.
                proven_owner(job)
                preserve(owner, evidence, report)
                vm_control.save(evidence / "cleanup.json", report)
                capture(owner, evidence, report)
                reclaim(owner, report)
        if not report["errors"]:
            report["status"] = "passed"
    except Exception as error:  # ruff: ignore[blind-except] - Persist all partial cleanup failures before returning nonzero.
        report["errors"].append(f"{type(error).__name__}: {error}")
    finally:
        report["finishedAt"] = candidate.now()
        vm_control.save(evidence / "cleanup.json", report)
        print(json.dumps(report, indent=2))
    if report["status"] != "passed":
        message = f"Cleanup incomplete; retained details: {evidence}"
        raise RuntimeError(message)


if __name__ == "__main__":
    cleanup(Path(sys.argv[1]))
