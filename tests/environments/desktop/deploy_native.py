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
GUEST = (
    Path(__file__)
    .with_name("deploy_native_guest.py")
    .read_text(encoding="utf-8")
)
PROBE = SESSION + shlex.join(["python3", "-c", GUEST, "probe"])
JOURNAL = (
    SESSION
    + """systemctl --user status omarchy-quickshare.service --no-pager;
journalctl --user -u omarchy-quickshare.service -b -n 150 --no-pager
"""
)


def _artifact(
    selected: Path | None, component: str = "all"
) -> tuple[Path, dict[str, Any]]:
    roles = build_native.components(component)
    if selected is None:
        pointer = (
            "latest.json" if component == "all" else f"latest-{component}.json"
        )
        selected = Path(
            json.loads(
                (build_native.BUILDS / pointer).read_text(encoding="utf-8")
            )["artifact"]
        )
    selected = selected.resolve(strict=True)
    metadata = json.loads((selected / "build.json").read_text(encoding="utf-8"))
    _validate_metadata(metadata, component, roles)
    _validate_bytes(selected, metadata, roles)
    return selected, metadata


def _validate_metadata(
    metadata: dict[str, Any], component: str, roles: tuple[str, ...]
) -> None:
    if metadata.get("schemaVersion") != build_native.SCHEMA_VERSION:
        message = "Unsupported native build schema; rebuild separate components"
        raise RuntimeError(message)
    if (
        metadata.get("status") != "built"
        or type(metadata.get("controlProtocol")) is not int
        or metadata["controlProtocol"] <= 0
    ):
        message = "Artifact has no successful versioned native build record"
        raise RuntimeError(message)
    records = metadata.get("components", {})
    if (
        not isinstance(records, dict)
        or not set(roles) <= records.keys()
        or not records.keys() <= build_native.COMPONENTS.keys()
    ):
        message = f"Native artifact lacks requested components: {component}"
        raise RuntimeError(message)


def _validate_bytes(
    selected: Path, metadata: dict[str, Any], roles: tuple[str, ...]
) -> None:
    records = metadata["components"]
    for role in roles:
        record = records[role]
        name = build_native.COMPONENTS[role]["file"]
        if record.get("file") != name or build_native.digest(
            selected / name
        ) != record.get("sha256"):
            message = f"Artifact bytes differ from build record: {role}"
            raise RuntimeError(message)
    if "daemon" in roles:
        for path in (selected / build_native.UNIT.name, build_native.UNIT):
            if build_native.digest(path) != metadata.get("unitSha256"):
                message = (
                    "Artifact service differs from shipped service; rebuild"
                )
                raise RuntimeError(message)


def _guest(state: dict[str, Any], command: str, log: Path) -> str:
    result = vm_control.ssh(state, command, check=False)
    with log.open("a", encoding="utf-8") as stream:
        stream.write(result.stdout + result.stderr)
    result.check_returncode()
    return result.stdout


def _copy(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Copy selected roles using owned transport and evidence.
    state: dict[str, Any], artifact: Path, attempt: Path, component: str
) -> str:
    stage = f".cache/omarchy-quickshare-deploy/{attempt.name}"
    _guest(state, f"mkdir -p -- {shlex.quote(stage)}", attempt / "copy.log")
    with (attempt / "copy.log").open("a", encoding="utf-8") as log:
        vm_control.run(
            [
                "scp",
                "-F",
                Path(state["directory"]) / "ssh.config",
                *[
                    artifact / build_native.COMPONENTS[role]["file"]
                    for role in build_native.components(component)
                ],
                *(
                    [artifact / build_native.UNIT.name]
                    if "daemon" in build_native.components(component)
                    else []
                ),
                f"desktop:{stage}/",
            ],
            stdout=log,
            stderr=log,
            timeout=120,
        )
    return stage


def _identity(  # ruff: ignore[too-many-arguments] - Bind a protocol probe to its guest and evidence file.
    state: dict[str, Any],
    attempt: Path,
    name: str,
    *,
    version: int | None = None,
) -> dict[str, Any]:
    return json.loads(
        _guest(
            state,
            "bash -lc "
            + shlex.quote(
                SESSION
                + shlex.join([
                    "python3",
                    "-c",
                    GUEST,
                    "identity",
                    *([str(version)] if version is not None else []),
                ])
            ),
            attempt / name,
        )
    )


def _check_identity(
    after: dict[str, Any], before: dict[str, Any], expected: dict[str, Any]
) -> None:
    if after["runtime"] != before["runtime"]:
        message = "Desktop or shell process changed during native deployment"
        raise RuntimeError(message)
    for role, record in expected.items():
        if after["components"][role]["installedSha256"] != record["sha256"]:
            message = f"Installed {role} differs from selected artifact"
            raise ValueError(message)
    daemon = after["components"]["daemon"]
    if "daemon" in expected and (
        daemon["activeState"] != "active"
        or daemon["loadedSha256"] != daemon["installedSha256"]
    ):
        message = "Selected daemon is not active with the installed executable"
        raise ValueError(message)
    for role in build_native.COMPONENTS.keys() - expected.keys():
        if before["components"].get(role) != after["components"].get(role):
            message = f"Unselected {role} identity changed"
            raise RuntimeError(message)


def _verify(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Compare pre/post identity against the selected immutable build.
    state: dict[str, Any],
    metadata: dict[str, Any],
    attempt: Path,
    component: str,
    before: dict[str, Any],
) -> dict[str, Any]:
    import time  # ruff: ignore[import-outside-top-level] - Readiness polling only.

    deadline = time.monotonic() + 60
    last = "No readiness attempt completed"
    while time.monotonic() < deadline:
        try:
            after = _identity(
                state,
                attempt,
                "readiness.log",
                version=metadata["controlProtocol"],
            )
            expected = {
                role: metadata["components"][role]
                for role in build_native.components(component)
            }
            _check_identity(after, before, expected)
            daemon = after["components"]["daemon"]
            return {
                "health": "available"
                if daemon["activeState"] == "active"
                else daemon["activeState"],
                "components": after["components"],
                "runtimeBefore": before["runtime"],
                "runtimeAfter": after["runtime"],
                **{
                    key: daemon[key]
                    for key in (
                        "mainPid",
                        "executable",
                        "loadedSha256",
                        "installedSha256",
                    )
                    if key in daemon
                },
            }
        except (ValueError, OSError, subprocess.SubprocessError) as error:
            last = str(error)
        time.sleep(1)
    message = f"Native deployment did not become ready: {last}"
    raise RuntimeError(message)


def _deploy(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Carry one selected role set across copy, install and verification.
    state: dict[str, Any], artifact: Path, attempt: Path, component: str = "all"
) -> dict[str, Any]:
    artifact, metadata = _artifact(artifact, component)
    diagnostics.desktop_ready(state, 30)
    before = _identity(
        state,
        attempt,
        "before.log",
        version=metadata["controlProtocol"] if component == "cli" else None,
    )
    roles = build_native.components(component)
    stage = _copy(state, artifact, attempt, component)
    script = SESSION + shlex.join([
        "python3",
        "-c",
        GUEST,
        "install",
        stage,
        json.dumps({
            "controlProtocol": metadata["controlProtocol"],
            "components": {
                role: {"sha256": metadata["components"][role]["sha256"]}
                for role in roles
            },
            "unitSha256": metadata.get("unitSha256"),
        }),
        json.dumps(roles),
    ])
    _guest(state, "bash -lc " + shlex.quote(script), attempt / "install.log")
    return {
        "artifact": str(artifact),
        "build": metadata,
        "guestStage": stage,
        "component": component,
        **_verify(state, metadata, attempt, component, before),
    }


def _running(owner: Path) -> dict[str, Any]:
    state = lab_lifecycle.active(owner)
    if not state or not vm_control.running(state):
        message = "Selected lab is not running; deployment never starts a VM"
        raise RuntimeError(message)
    return state


def _attempt(owner: Path, selected: Path | None, component: str) -> None:
    attempt = owner / "deployments" / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    report = {
        "deploymentId": attempt.name,
        "lab": owner.name,
        "startedAt": datetime.now(UTC).isoformat(),
        "component": component,
        "status": "failed",
    }
    state = None
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Persist selected artifact intent before any guest mutation.
        artifact, _ = _artifact(selected, component)
        state = _running(owner)
        report.update({"guestUuid": state["uuid"], "qemuPid": state["pid"]})
        report["artifact"] = str(artifact)
        vm_control.save(attempt / "deployment.json", report)
        report.update(_deploy(state, artifact, attempt, component))
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
            _attempt(
                owner,
                options.artifact,
                os.environ.get("NATIVE_COMPONENT", "all"),
            )


if __name__ == "__main__":
    main()
