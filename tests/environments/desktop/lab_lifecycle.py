"""Retained named lab ownership, writable storage and lifecycle dispatch."""  # ruff: ignore[implicit-namespace-package] - Imported by the standalone environment script.

import fcntl
import json
import os
import re
import shutil
from pathlib import Path
from typing import Any

import diagnostics
import vm_control


def directory() -> Path:
    """Resolve the selected conservative lab identifier.

    Returns:
        The private directory owned by this name.

    Raises:
        RuntimeError: DESKTOP_LAB is not a conservative identifier.
    """
    name = os.environ.get("DESKTOP_LAB", "dev")
    if re.fullmatch(r"[a-z][a-z0-9-]{0,31}", name) is None:
        message = "DESKTOP_LAB must match [a-z][a-z0-9-]{0,31}"
        raise RuntimeError(message)
    return vm_control.CACHE / "labs" / name


def active(owner: Path) -> dict[str, Any] | None:
    """Read only state belonging to the selected lab directory.

    Returns:
        Validated state or None for a new lab.

    Raises:
        RuntimeError: State points outside the selected owner.
    """
    state = vm_control.active(owner / "state.json")
    if state is None:
        return None
    expected = {
        "directory": str(owner),
        "stateFile": str(owner / "state.json"),
        "disk": str(owner / "desktop.qcow2"),
        "firmware": str(owner / "vars.fd"),
    }
    if any(state.get(key) != value for key, value in expected.items()):
        message = f"Lab ownership mismatch; retained evidence: {owner}"
        raise RuntimeError(message)
    return state


def _compatible(state: dict[str, Any], fingerprint: str) -> None:
    if state["fingerprint"] != fingerprint:
        message = (
            "Lab uses different preparation inputs; retained state is not "
            "replaced. Prepare current inputs and choose a new DESKTOP_LAB."
        )
        raise RuntimeError(message)
    if vm_control.running(state):
        requested = {
            "cpus": vm_control.number("DESKTOP_CPUS", 1, 1),
            "memoryMiB": vm_control.number("DESKTOP_MEMORY_MIB", 2048, 1024),
        }
        if any(
            state["resources"][key] != value for key, value in requested.items()
        ):
            message = (
                "Running allocation differs; explicitly stop before resizing"
            )
            raise RuntimeError(message)


def _storage(owner: Path, base: Path) -> None:
    # Never adopt partial or historical writable storage without its state.
    unexpected = [
        path for path in owner.iterdir() if path.name != "operation.lock"
    ]
    if unexpected:
        message = (
            f"Unowned lab files retained; inspect {owner}, choose another name"
        )
        raise RuntimeError(message)
    vm_control.run([
        "qemu-img",
        "create",
        "-f",
        "qcow2",
        "-F",
        "qcow2",
        "-b",
        base / "base.qcow2",
        owner / "desktop.qcow2",
    ])
    shutil.copyfile(base / "vars.fd", owner / "vars.fd")
    shutil.copyfile(base / "known_hosts", owner / "known_hosts")


def _retained(state: dict[str, Any], base: Path) -> None:
    for field in ("disk", "firmware"):
        if not Path(state[field]).is_file():
            message = f"Retained lab artifact missing: {state[field]}"
            raise RuntimeError(message)
    owner = Path(state["directory"])
    if (owner / "known_hosts").read_bytes() != (
        base / "known_hosts"
    ).read_bytes():
        message = "Retained SSH host pin differs from prepared base"
        raise RuntimeError(message)
    chain = vm_control.output([
        "qemu-img",
        "info",
        "--output=json",
        owner / "desktop.qcow2",
    ])
    information = json.loads(chain)
    if information.get("full-backing-filename") != str(base / "base.qcow2"):
        message = "Retained overlay backing image differs from prepared base"
        raise RuntimeError(message)


def _launch_retained(
    owner: Path,
    preparation: tuple[str, dict[str, Any], Path, Path],
    state: dict[str, Any] | None,
) -> dict[str, Any]:
    fingerprint, _, code, _ = preparation
    descriptor = os.open(
        vm_control.CACHE / "bases.lock",
        os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
        0o600,
    )
    with os.fdopen(descriptor, "a") as bases:
        fcntl.flock(bases, fcntl.LOCK_SH | fcntl.LOCK_NB)
        base = vm_control.CACHE / "bases" / fingerprint
        prepared = vm_control.active(base / "prepared.json")
        if not prepared or prepared.get("fingerprint") != fingerprint:
            message = "No compatible prepared base; run make desktop-prepare"
            raise RuntimeError(message)
        allocation = vm_control.resources()
        if state:
            _retained(state, base)
        else:
            _storage(owner, base)
        return vm_control.launch(
            owner,
            allocation,
            disk=owner / "desktop.qcow2",
            variables=owner / "vars.fd",
            key=base / "id_ed25519",
            fingerprint=fingerprint,
            code=code,
            previous=state,
        )


def start(
    owner: Path, preparation: tuple[str, dict[str, Any], Path, Path]
) -> None:
    """Reuse a healthy owner or resume its retained writable disk."""
    fingerprint = preparation[0]
    state = active(owner)
    if state:
        _compatible(state, fingerprint)
    if vm_control.running(state):
        diagnostics.desktop_ready(
            state, vm_control.number("DESKTOP_BOOT_TIMEOUT", 600, 1)
        )
        print(f"Reusing healthy lab {owner.name}: {owner}")
        return
    state = _launch_retained(owner, preparation, state)
    diagnostics.desktop_ready(
        state, vm_control.number("DESKTOP_BOOT_TIMEOUT", 600, 1)
    )
    state["status"] = "ready"
    vm_control.save(owner / "state.json", state)
    print(
        f"Desktop lab {owner.name} ready. "
        f"SSH: ssh -F {owner}/ssh.config desktop"
    )


def report(owner: Path, command: str) -> None:
    """Expose the selected owner's diagnostics without requiring readiness."""
    print(f"Lab: {owner.name}; state: {owner / 'state.json'}")
    state = active(owner)
    if command == "logs":
        diagnostics.logs(state)
    else:
        diagnostics.inspect(state, capture=command == "inspect")
