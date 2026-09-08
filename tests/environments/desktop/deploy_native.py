"""Atomically replace the native application in one healthy owned lab."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import argparse
import fcntl
import json
import os
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Capture bounded tool failures.
import sys
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import build_native
import diagnostics
import lab_lifecycle
import vm_control

SESSION = """export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
"""
INSTALL = (
    SESSION
    + """set -Eeuo pipefail
stage=$1
expected=$2
unit_hash=$3
name=omarchy-quickshare
binary="$HOME/.local/bin/$name"
unit="$HOME/.config/systemd/user/$name.service"
test "$(sha256sum "$stage/$name" | cut -d ' ' -f 1)" = "$expected"
test "$(sha256sum "$stage/$name.service" | cut -d ' ' -f 1)" = "$unit_hash"
mkdir -p "$(dirname "$binary")" "$(dirname "$unit")"
binary_tmp=$(mktemp "${binary}.XXXXXXXX")
unit_tmp=$(mktemp "${unit}.XXXXXXXX")
trap 'rm -f -- "$binary_tmp" "$unit_tmp"' EXIT
install -m 755 "$stage/$name" "$binary_tmp"
install -m 644 "$stage/$name.service" "$unit_tmp"
# Check loadability without starting a daemon.
timeout 10 "$binary_tmp" --version >"$stage/version.log" 2>&1
mv -fT -- "$binary_tmp" "$binary"
mv -fT -- "$unit_tmp" "$unit"
systemctl --user daemon-reload
systemctl --user restart "$name.service"
"""
)
PROBE = (
    SESSION
    + """set -Eeuo pipefail
name=omarchy-quickshare
binary="$HOME/.local/bin/$name"
systemctl --user is-active --quiet "$name.service"
pid=$(systemctl --user show "$name.service" -p MainPID --value)
test "$pid" -gt 0
timeout 5 "$binary" health
sha256sum "/proc/$pid/exe"
readlink "/proc/$pid/exe"
printf '%s\\n' "$pid"
test "$(systemctl --user show "$name.service" -p MainPID --value)" = "$pid"
systemctl --user is-active --quiet "$name.service"
"""
)
JOURNAL = (
    SESSION
    + """systemctl --user status omarchy-quickshare.service --no-pager;
journalctl --user -u omarchy-quickshare.service -b -n 150 --no-pager
"""
)


def _artifact(selected: Path | None) -> tuple[Path, dict[str, Any]]:
    if selected is None:
        selected = Path(
            json.loads(
                (build_native.BUILDS / "latest.json").read_text(
                    encoding="utf-8"
                )
            )["artifact"]
        )
    selected = selected.resolve(strict=True)
    metadata = json.loads((selected / "build.json").read_text(encoding="utf-8"))
    if metadata.get("status") != "built":
        message = "Artifact has no successful native build record"
        raise RuntimeError(message)
    for name, field in (
        (build_native.NAME, "sha256"),
        (build_native.UNIT.name, "unitSha256"),
    ):
        if build_native.digest(selected / name) != metadata.get(field):
            message = f"Artifact bytes differ from build record: {name}"
            raise RuntimeError(message)
    if build_native.digest(build_native.UNIT) != metadata["unitSha256"]:
        message = (
            "Artifact service differs from current shipped service; rebuild"
        )
        raise RuntimeError(message)
    return selected, metadata


def _guest(state: dict[str, Any], command: str, log: Path) -> str:
    result = vm_control.ssh(state, command, check=False)
    with log.open("a", encoding="utf-8") as stream:
        stream.write(result.stdout + result.stderr)
    result.check_returncode()
    return result.stdout


def _copy(state: dict[str, Any], artifact: Path, attempt: Path) -> str:
    stage = f".cache/omarchy-quickshare-deploy/{attempt.name}"
    _guest(state, f"mkdir -p -- {shlex.quote(stage)}", attempt / "copy.log")
    with (attempt / "copy.log").open("a", encoding="utf-8") as log:
        vm_control.run(
            [
                "scp",
                "-F",
                Path(state["directory"]) / "ssh.config",
                artifact / build_native.NAME,
                artifact / build_native.UNIT.name,
                f"desktop:{stage}/",
            ],
            stdout=log,
            stderr=log,
            timeout=120,
        )
    return stage


def _verify(
    state: dict[str, Any], metadata: dict[str, Any], attempt: Path
) -> dict[str, Any]:
    # SSH and public readiness calls each have a timeout.
    import time  # ruff: ignore[import-outside-top-level] - Readiness polling only.

    deadline = time.monotonic() + 60
    last = "No public readiness attempt completed"
    while time.monotonic() < deadline:
        try:
            result = _guest(
                state,
                "bash -lc " + shlex.quote(PROBE),
                attempt / "readiness.log",
            )
            health, checksum, executable, pid = result.strip().splitlines()
            loaded = checksum.split()[0]
            if health == "available" and loaded == metadata["sha256"]:
                return {
                    "health": health,
                    "loadedSha256": loaded,
                    "executable": executable,
                    "mainPid": int(pid),
                }
            last = f"Readiness or loaded executable mismatch: {result.strip()}"
        except (ValueError, OSError, subprocess.SubprocessError) as error:
            last = str(error)
        time.sleep(1)
    message = f"Native deployment did not become ready: {last}"
    raise RuntimeError(message)


def _deploy(
    state: dict[str, Any], artifact: Path, attempt: Path
) -> dict[str, Any]:
    artifact, metadata = _artifact(artifact)
    diagnostics.desktop_ready(state, 30)
    stage = _copy(state, artifact, attempt)
    command = (
        "bash -lc "
        + shlex.quote(INSTALL)
        + " -- "
        + " ".join(
            map(
                shlex.quote, (stage, metadata["sha256"], metadata["unitSha256"])
            )
        )
    )
    _guest(state, command, attempt / "install.log")
    return {
        "artifact": str(artifact),
        "build": metadata,
        "guestStage": stage,
        **_verify(state, metadata, attempt),
    }


def _running(owner: Path) -> dict[str, Any]:
    state = lab_lifecycle.active(owner)
    if not state or not vm_control.running(state):
        message = "Selected lab is not running; deployment never starts a VM"
        raise RuntimeError(message)
    return state


def _attempt(owner: Path, selected: Path | None) -> None:
    attempt = owner / "deployments" / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    report = {
        "deploymentId": attempt.name,
        "lab": owner.name,
        "startedAt": datetime.now(UTC).isoformat(),
        "status": "failed",
    }
    state = None
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Persist selected artifact intent before any guest mutation.
        artifact, _ = _artifact(selected)
        state = _running(owner)
        report.update({"guestUuid": state["uuid"], "qemuPid": state["pid"]})
        report["artifact"] = str(artifact)
        vm_control.save(attempt / "deployment.json", report)
        report.update(_deploy(state, artifact, attempt))
        report["status"] = "ready"
    except (
        OSError,
        ValueError,
        KeyError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        report["error"] = str(error)
        print(
            f"Native deployment failed; retained lab and evidence: {attempt}",
            file=sys.stderr,
        )
        raise
    finally:
        report["finishedAt"] = datetime.now(UTC).isoformat()
        vm_control.save(attempt / "deployment.json", report)
        if state and vm_control.running(state):
            _journal(state, attempt)
    print(json.dumps(report, indent=2))


def _journal(state: dict[str, Any], attempt: Path) -> None:
    try:
        result = vm_control.ssh(
            state, "bash -lc " + shlex.quote(JOURNAL), check=False
        )
        (attempt / "journal.log").write_text(
            result.stdout + result.stderr, encoding="utf-8"
        )
    except (OSError, subprocess.SubprocessError) as error:
        (attempt / "journal-error.log").write_text(str(error), encoding="utf-8")


def main() -> None:
    """Deploy an artifact under the selected lab's lifecycle lock."""
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--artifact",
        type=Path,
        help="Native build directory; default: latest successful build",
    )
    options = parser.parse_args()
    owner = lab_lifecycle.directory()
    # Do not create lab directories or reinterpret an absent owner as a new lab.
    with (owner / "operation.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        descriptor = os.open(
            build_native.BUILDS / "artifacts.lock",
            os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
            0o600,
        )
        with os.fdopen(descriptor, "a") as artifacts:
            fcntl.flock(artifacts, fcntl.LOCK_SH | fcntl.LOCK_NB)
            _attempt(owner, options.artifact)


if __name__ == "__main__":
    main()
