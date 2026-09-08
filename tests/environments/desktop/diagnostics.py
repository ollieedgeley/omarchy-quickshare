"""Live desktop health and retained diagnostics independent of preparation."""  # ruff: ignore[implicit-namespace-package] - Imported by the standalone environment script.

import json
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Catch bounded SSH failures.
import time
from pathlib import Path
from typing import Any

import vm_control

DESKTOP_PROBE = """set -e
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
test -S "$XDG_RUNTIME_DIR/bus"
systemctl --user show-environment >/dev/null
export HYPRLAND_INSTANCE_SIGNATURE=$(hyprctl instances -j | python -c '
import json,sys; a=json.load(sys.stdin); assert a; print(a[0]["instance"])')
hyprctl monitors -j | python -c '
import json,sys; a=json.load(sys.stdin)
assert any(not x.get("disabled",False) for x in a)'
omarchy-shell shell ping
"""


def desktop_ready(state: dict[str, Any], timeout: float) -> None:
    """Wait for the user bus, compositor output and shell IPC.

    Raises:
        RuntimeError: The process exits or desktop readiness times out.
    """
    deadline = time.monotonic() + timeout
    last = "No readiness attempt completed"
    while time.monotonic() < deadline:
        if not vm_control.running(state):
            message = "Guest exited before desktop readiness"
            raise RuntimeError(message)
        try:
            result = vm_control.ssh(
                state, "bash -lc " + shlex.quote(DESKTOP_PROBE), check=False
            )
            qemu = vm_control.qmp(state, "query-status")
            if result.returncode == 0 and qemu.get("status") == "running":
                vm_control.screenshot(state)
                return
            last = result.stderr.strip() or result.stdout.strip()
        except (
            OSError,
            ValueError,
            RuntimeError,
            subprocess.SubprocessError,
        ) as error:
            last = str(error)
        time.sleep(5)
    message = f"Desktop not ready: {last}; inspect {state['directory']}"
    raise RuntimeError(message)


def _probe(state: dict[str, Any], report: dict[str, Any]) -> None:
    try:
        report["qemu"] = vm_control.qmp(state, "query-status")
    except (OSError, ValueError, RuntimeError) as error:
        report["qmpError"] = str(error)
    try:
        result = vm_control.ssh(
            state, "bash -lc " + shlex.quote(DESKTOP_PROBE), check=False
        )
        report["desktopReady"] = result.returncode == 0
        report["readinessError"] = (
            result.stderr.strip() or result.stdout.strip()
        )
    except (OSError, subprocess.SubprocessError) as error:
        report["readinessError"] = str(error)
    if (
        report["desktopReady"]
        and report.get("qemu", {}).get("status") == "running"
    ):
        report["health"] = "ready"


def inspect(state: dict[str, Any] | None, *, capture: bool = False) -> None:
    """Print ownership and independent health probes even when one fails."""
    if state is None:
        print(json.dumps({"health": "absent"}, indent=2))
        return
    report = dict(state)
    report["recordedStatus"] = report.pop("status", None)
    report["running"] = vm_control.running(state)
    report["desktopReady"] = False
    report["health"] = "unhealthy" if report["running"] else "stopped"
    directory = Path(state["directory"])
    report["sshCommand"] = (
        f"ssh -F {shlex.quote(str(directory / 'ssh.config'))} desktop"
    )
    report["vncSocket"] = str(directory / "vnc.sock")
    report["qmpSocket"] = str(directory / "qmp.sock")
    report["logs"] = [
        str(directory / name) for name in ("serial.log", "qemu.log")
    ]
    if report["running"]:
        _probe(state, report)
    if capture and report["running"]:
        try:
            report["screenshot"] = str(vm_control.screenshot(state))
        except (OSError, ValueError, RuntimeError) as error:
            report["screenshotError"] = str(error)
    print(json.dumps(report, indent=2))


def logs(state: dict[str, Any] | None) -> None:
    """Print bounded retained host logs and guest journal when reachable."""
    if state is None:
        print("No retained guest logs")
        return
    directory = Path(state["directory"])
    for name in ("serial.log", "qemu.log"):
        path = directory / name
        print(f"=== {path} (last 64 KiB) ===")
        try:
            with path.open("rb") as stream:
                stream.seek(max(0, path.stat().st_size - 65536))
                print(stream.read().decode(errors="replace"))
        except OSError as error:
            print(f"Unavailable: {error}")
    if vm_control.running(state):
        try:
            result = vm_control.ssh(
                state,
                "journalctl --user -b -n 100 --no-pager; "
                "journalctl -b -n 50 --no-pager",
                check=False,
            )
            print(result.stdout)
            print(result.stderr)
        except (OSError, subprocess.SubprocessError) as error:
            print(f"Guest journal unavailable: {error}")
