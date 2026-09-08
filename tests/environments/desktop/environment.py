#!/usr/bin/env python3
"""Private QEMU desktop: prepare, start, inspect, stop and SSH.

Requires Python 3, QEMU/KVM, OVMF, curl, OpenSSH, openssl, dosfstools,
mtools, ImageMagick and tesseract. MCOPY and DESKTOP_OVMF_CODE/VARS may
point at private tools. DESKTOP_CPUS=1, DESKTOP_MEMORY_MIB=2048 and
DESKTOP_HOST_HEADROOM_MIB=1024 leave host capacity; headroom cannot be <512.
DESKTOP_BOOT_TIMEOUT=600 and DESKTOP_INSTALL_TIMEOUT=3600 bound readiness.

Preparation trusts first contact only on the owned QEMU loopback forward.
Each install probe uses disposable host keys; only a successful probe of the
installed btrfs root pins its key. Runtime SSH uses that pin, including
overlays.
"""

import fcntl
import hashlib
import json
import os
import shlex
import shutil
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Used only to catch subprocess failures.
import sys
import time
import uuid
from pathlib import Path
from typing import Any

import diagnostics
import lab_lifecycle
import retention_bases
import vm_control
from seed import build


def digest(path: str | Path) -> str:
    """Hash a preparation input without loading it into memory.

    Returns:
        The SHA-256 hexadecimal digest.
    """
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def firmware() -> tuple[Path, Path]:
    """Locate the configured or distribution-provided OVMF pair.

    Returns:
        Firmware code and variable-template paths.

    Raises:
        RuntimeError: No supported firmware pair is installed.
    """
    if os.environ.get("DESKTOP_OVMF_CODE") and os.environ.get(
        "DESKTOP_OVMF_VARS"
    ):
        return Path(os.environ["DESKTOP_OVMF_CODE"]), Path(
            os.environ["DESKTOP_OVMF_VARS"]
        )
    for directory, suffix in [
        ("/usr/share/edk2/x64", ".4m"),
        ("/usr/share/OVMF", "_4M"),
        ("/usr/share/OVMF", ""),
        ("/usr/share/edk2/ovmf", ""),
    ]:
        code, variables = [
            Path(directory) / f"OVMF_{name}{suffix}.fd"
            for name in ("CODE", "VARS")
        ]
        if code.is_file() and variables.is_file():
            return code, variables
    message = "OVMF missing; set DESKTOP_OVMF_CODE and DESKTOP_OVMF_VARS"
    raise RuntimeError(message)


def identity() -> tuple[str, dict[str, Any], Path, Path]:
    """Fingerprint heterogeneous manifest and tool metadata for base reuse.

    Returns:
        The fingerprint, input metadata, firmware and variable template.
    """
    code, variables = firmware()
    inputs = {
        "manifest": vm_control.MANIFEST,
        "preparation": {
            name: digest(vm_control.SOURCE / name)
            for name in (
                "environment.py",
                "vm_control.py",
                "seed.py",
                "lab_lifecycle.py",
                "diagnostics.py",
            )
        },
        "firmware": {"code": digest(code), "vars": digest(variables)},
        "qemu": vm_control.output([
            "qemu-system-x86_64",
            "--version",
        ]).splitlines()[0],
    }
    key = hashlib.sha256(
        json.dumps(inputs, sort_keys=True).encode()
    ).hexdigest()
    return key, inputs, code, variables


def _prepared(base: Path) -> bool:
    """Check that a published base still has its required files.

    Returns:
        Whether the prepared base exists and is complete.

    Raises:
        RuntimeError: A published base is incomplete.
    """
    if not (base / "prepared.json").exists():
        return False
    for name in ("base.qcow2", "vars.fd", "id_ed25519", "known_hosts"):
        if not (base / name).is_file():
            message = f"Incomplete prepared base: {base / name}"
            raise RuntimeError(message)
    print(f"Compatible prepared base: {base}")
    return True


def _installation_iso() -> Path:
    """Fetch the pinned ISO and check both partial and cached downloads.

    Returns:
        The verified installation ISO path.

    Raises:
        RuntimeError: The downloaded or cached ISO has the wrong digest.
    """
    iso = (
        vm_control.CACHE
        / "downloads"
        / f"omarchy-{vm_control.MANIFEST['iso']['version']}.iso"
    )
    iso.parent.mkdir(parents=True, exist_ok=True)
    if not iso.exists():
        partial = iso.with_suffix(".iso.partial")
        vm_control.run([
            "curl",
            "--fail",
            "--location",
            "--retry",
            "3",
            "--continue-at",
            "-",
            "--output",
            partial,
            vm_control.MANIFEST["iso"]["url"],
        ])
        if digest(partial) != vm_control.MANIFEST["iso"]["sha256"]:
            message = f"ISO checksum mismatch: {partial}"
            raise RuntimeError(message)
        partial.replace(iso)
    if digest(iso) != vm_control.MANIFEST["iso"]["sha256"]:
        message = f"ISO checksum mismatch: {iso}"
        raise RuntimeError(message)
    return iso


def prepare() -> None:
    """Install and publish a reusable desktop base after host preflight.

    Raises:
        RuntimeError: Another owned guest is running.
    """
    fingerprint, inputs, code, template = identity()
    base = vm_control.CACHE / "bases" / fingerprint
    if _prepared(base):
        return
    if vm_control.running(
        vm_control.active(vm_control.CACHE / "preparation.json")
    ):
        message = (
            "Preparation guest still running; use desktop-prepare-inspect/stop"
        )
        raise RuntimeError(message)
    vm_control.resources()
    iso = _installation_iso()
    directory = vm_control.CACHE / "preparations" / str(uuid.uuid4())
    directory.mkdir(parents=True)
    key = directory / "id_ed25519"
    vm_control.run([
        "ssh-keygen",
        "-q",
        "-t",
        "ed25519",
        "-N",
        "",
        "-C",
        "private-desktop",
        "-f",
        key,
    ])
    seed = build(
        directory,
        vm_control.MANIFEST,
        key.with_suffix(".pub").read_text(encoding="utf-8").strip(),
    )
    disk = directory / "base.qcow2"
    vm_control.run([
        "qemu-img",
        "create",
        "-f",
        "qcow2",
        disk,
        f"{vm_control.MANIFEST['diskGiB']}G",
    ])
    variables = directory / "vars.fd"
    shutil.copyfile(template, variables)
    state = vm_control.launch(
        directory,
        vm_control.resources(),
        disk=disk,
        variables=variables,
        key=key,
        fingerprint=fingerprint,
        code=code,
        state_file=vm_control.CACHE / "preparation.json",
        extras=[
            "-drive",
            f"file={iso},media=cdrom,if=none,format=raw,id=cd0",
            "-device",
            "ide-cd,drive=cd0,bootindex=2",
            "-drive",
            f"file={seed},format=raw,if=none,id=seed",
            "-device",
            "usb-storage,drive=seed",
        ],
    )
    vm_control.wait_ssh(
        state,
        vm_control.number("DESKTOP_INSTALL_TIMEOUT", 3600, 1),
        installing=True,
    )
    setup = (
        "set -e; mkdir -p /etc/sddm.conf.d; "
        "printf '[Autologin]\\nUser=omarchy\\nSession=omarchy\\n' "
        "> /etc/sddm.conf.d/50-test-desktop.conf; systemctl restart sddm"
    )
    vm_control.ssh(
        state, "printf '%s\\n' omarchy | sudo -S bash -c " + shlex.quote(setup)
    )
    diagnostics.desktop_ready(
        state, vm_control.number("DESKTOP_BOOT_TIMEOUT", 600, 1)
    )
    runtime = vm_control.ssh(
        state,
        "bash -lc "
        + shlex.quote(
            "set -e; cat /etc/os-release; uname -r; "
            "pacman -Q omarchy omarchy-settings hyprland; "
            "command -v omarchy-shell; "
            "command -v omarchy-plugin-validate; pacman -Q"
        ),
    ).stdout
    (directory / "runtime.txt").write_text(runtime, encoding="utf-8")
    vm_control.stop(state, force=False)
    base.mkdir(parents=True, exist_ok=True)
    for name in (
        "base.qcow2",
        "vars.fd",
        "id_ed25519",
        "id_ed25519.pub",
        "known_hosts",
        "runtime.txt",
    ):
        shutil.move(directory / name, base / name)
    (base / "base.qcow2").chmod(0o400)
    vm_control.save(
        base / "prepared.pending.json",
        {
            "fingerprint": fingerprint,
            "inputs": inputs,
            "runtime": runtime,
            "preparedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        },
    )
    retention_bases.publish(base)
    print(f"Prepared and cleanly shut down: {base}")


def main() -> None:
    """Dispatch diagnostics freely and serialize each mutable owner separately.

    Raises:
        RuntimeError: The requested command is unknown.
    """
    os.umask(0o077)
    command = sys.argv[1] if len(sys.argv) > 1 else "help"
    if command == "help":
        print(
            __doc__ + "\nCommands: prepare|start|status|logs|inspect|stop"
            "|prepare-inspect|prepare-stop"
        )
        return
    if command not in {
        "prepare",
        "start",
        "status",
        "logs",
        "inspect",
        "stop",
        "prepare-inspect",
        "prepare-stop",
    }:
        message = f"Unknown command: {command}"
        raise RuntimeError(message)
    owner = lab_lifecycle.directory()
    owner.mkdir(parents=True, exist_ok=True)
    if command == "prepare-inspect":
        diagnostics.inspect(
            vm_control.active(vm_control.CACHE / "preparation.json"),
            capture=True,
        )
        return
    if command in {"status", "logs", "inspect"}:
        lab_lifecycle.report(owner, command)
        return
    _mutate(owner, command)


def _mutate(owner: Path, command: str) -> None:
    preparation = command.startswith("prepare")
    lock_path = (
        vm_control.CACHE / "prepare.lock"
        if preparation
        else owner / "operation.lock"
    )
    with lock_path.open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        _dispatch(owner, command)


def _dispatch(owner: Path, command: str) -> None:
    if command == "prepare":
        prepare()
    elif command == "start":
        lab_lifecycle.start(owner, identity())
    else:
        state = (
            vm_control.active(vm_control.CACHE / "preparation.json")
            if command == "prepare-stop"
            else lab_lifecycle.active(owner)
        )
        vm_control.stop(state)
        print("Selected guest stopped; writable state and diagnostics retained")


if __name__ == "__main__":
    try:
        main()
    except (
        OSError,
        ValueError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        print(f"desktop: {error}", file=sys.stderr)
        print(
            "Guest and diagnostic files are retained. "
            "Inspect or explicitly stop the owned guest.",
            file=sys.stderr,
        )
        sys.exit(1)
