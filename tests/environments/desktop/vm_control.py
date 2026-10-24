"""Owned QEMU process, private SSH transport and desktop readiness."""  # ruff: ignore[implicit-namespace-package] - Imported by the standalone environment script.

from __future__ import annotations

import itertools
import json
import os
import socket
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Host tools run through explicit argv.
import tempfile
import time
import uuid
from pathlib import Path
from typing import TYPE_CHECKING, Any

import resource_budget

if TYPE_CHECKING:
    from collections.abc import Iterable

SOURCE = Path(__file__).resolve().parent
ROOT = SOURCE.parents[2]
CACHE = resource_budget.CACHE
MANIFEST = json.loads((SOURCE / "environment.json").read_text(encoding="utf-8"))


def run(
    args: Iterable[str | int | Path], **kwargs: object
) -> subprocess.CompletedProcess:
    """Run host-tool argv, raising on failure.

    Returns:
        The completed host process.
    """
    return subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Callers construct argv, never shell text.
        list(map(str, args)), check=True, **kwargs
    )


def output(args: Iterable[str | int | Path], **kwargs: object) -> str:
    """Capture a host tool's text output.

    Returns:
        Standard output with surrounding whitespace removed.
    """
    return run(args, stdout=subprocess.PIPE, text=True, **kwargs).stdout.strip()


def save(path: Path, value: dict[str, Any]) -> None:
    """Atomically replace a JSON state or manifest dictionary."""
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    temporary.replace(path)


def number(name: str, default: int, minimum: int) -> int:
    """Read an integer environment override.

    Returns:
        The configured integer or its default.

    Raises:
        RuntimeError: If the override is below the required minimum.
    """
    value = int(os.environ.get(name, default))
    if value < minimum:
        message = f"{name} must be at least {minimum}"
        raise RuntimeError(message)
    return value


def resources() -> dict[str, int]:
    """Select allocations; launch performs joint admission with owner evidence.

    Returns:
        Guest allocations and the measured host memory headroom.

    Raises:
        RuntimeError: If allocation values or KVM access are invalid.
    """
    cpus = number("DESKTOP_CPUS", 1, 1)
    memory = number("DESKTOP_MEMORY_MIB", 2048, 1024)
    headroom = number("DESKTOP_HOST_HEADROOM_MIB", 1024, 512)
    available = (
        int(
            next(
                line.split()[1]
                for line in Path("/proc/meminfo")
                .read_text(encoding="utf-8")
                .splitlines()
                if line.startswith("MemAvailable:")
            )
        )
        // 1024
    )
    if not os.access("/dev/kvm", os.R_OK | os.W_OK):
        message = (
            "Readable/writable /dev/kvm required; no slow emulation fallback"
        )
        raise RuntimeError(message)
    return {
        "cpus": cpus,
        "memoryMiB": memory,
        "headroomMiB": headroom,
        "hostAvailableMiB": available,
    }


def active(path: Path) -> dict[str, Any] | None:
    """Load persisted state from an explicit owner.

    Returns:
        The state dictionary, or None when no state has been saved.
    """
    if path.exists():
        return json.loads(path.read_text(encoding="utf-8"))
    return None


def running(state: dict[str, Any] | None) -> bool:
    """Check PID ownership.

    Returns:
        Whether the recorded PID still belongs to this guest UUID.
    """
    if not state:
        return False
    try:
        pid = state.get("pid") or int(
            (Path(state["directory"]) / "qemu.pid").read_text(encoding="utf-8")
        )
        args = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
        return any(
            left == b"-uuid" and right == state["uuid"].encode()
            for left, right in itertools.pairwise(args)
        )
    except FileNotFoundError:
        return False


def qmp(
    state: dict[str, Any],
    command: str,
    arguments: dict[str, Any] | None = None,
) -> object:
    """Execute QMP against the owned process through its directory socket.

    Returns:
        The decoded QMP response payload.

    Raises:
        RuntimeError: If ownership is lost or QMP reports an error.
    """
    if not running(state):
        message = "Owned guest is not running"
        raise RuntimeError(message)
    directory = os.open(state["directory"], os.O_RDONLY | os.O_DIRECTORY)
    try:
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(10)
            connection.connect(f"/proc/self/fd/{directory}/qmp.sock")
            stream = connection.makefile("rwb")
            json.loads(stream.readline())
            for request in [
                {"execute": "qmp_capabilities"},
                {"execute": command, "arguments": arguments or {}},
            ]:
                stream.write(json.dumps(request).encode() + b"\n")
                stream.flush()
                while True:
                    reply = json.loads(stream.readline())
                    if "error" in reply:
                        message = f"QMP {command}: {reply['error']}"
                        raise RuntimeError(message)
                    if "return" in reply:
                        break
            return reply["return"]
    finally:
        os.close(directory)


def ssh(  # ruff: ignore[too-many-arguments] - SSH command and independent transport options.
    state: dict[str, Any],
    command: str,
    *,
    check: bool = True,
    known_hosts: Path | None = None,
) -> subprocess.CompletedProcess[str]:
    """Run a guest command with pinned SSH configuration by default.

    Returns:
        Captured guest output and the SSH exit status.
    """
    options = []
    if known_hosts is not None:
        options = [
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            f'UserKnownHostsFile="{known_hosts}"',
        ]
    return subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Explicit SSH argv, no host shell.
        [  # ruff: ignore[start-process-with-partial-path] - Use the host's installed OpenSSH client.
            "ssh",
            "-F",
            str(Path(state["directory"]) / "ssh.config"),
            *options,
            "desktop",
            command,
        ],
        capture_output=True,
        text=True,
        timeout=30,
        check=check,
    )


def _write_ssh_config(state: dict[str, Any]) -> None:
    directory = Path(state["directory"])
    (directory / "ssh.config").write_text(
        f"Host desktop\n HostName 127.0.0.1\n Port {state['sshPort']}\n"
        f" User {MANIFEST['user']}\n"
        f' IdentityFile "{state["sshKey"]}"\n IdentitiesOnly yes\n'
        " BatchMode yes\n ConnectTimeout 5\n"
        " StrictHostKeyChecking yes\n HostKeyAlias desktop\n"
        " GlobalKnownHostsFile /dev/null\n UpdateHostKeys no\n"
        f' UserKnownHostsFile "{directory / "known_hosts"}"\n',
        encoding="utf-8",
    )


def _qemu_args(
    state: dict[str, Any], extras: Iterable[str | int | Path]
) -> list[str | int | Path]:
    return [
        "qemu-system-x86_64",
        "-name",
        f"quickshare-desktop-{state['uuid']}",
        "-uuid",
        state["uuid"],
        "-machine",
        "q35,accel=kvm",
        "-cpu",
        "host",
        "-smp",
        state["resources"]["cpus"],
        "-m",
        state["resources"]["memoryMiB"],
        "-drive",
        f"if=pflash,format=raw,readonly=on,file={state['firmwareCode']}",
        "-drive",
        f"if=pflash,format=raw,file={state['firmware']}",
        "-drive",
        f"file={state['disk']},format=qcow2,if=none,id=disk0",
        "-device",
        "virtio-blk-pci,drive=disk0,bootindex=1",
        "-device",
        "virtio-vga",
        "-display",
        "none",
        "-vnc",
        "unix:vnc.sock",
        "-usb",
        "-device",
        "usb-tablet",
        "-netdev",
        f"user,id=net0,hostfwd=tcp:127.0.0.1:{state['sshPort']}-:22",
        "-device",
        "virtio-net-pci,netdev=net0",
        "-qmp",
        "unix:qmp.sock,server=on,wait=off",
        "-chardev",
        "file,id=serial0,path=serial.log,append=on",
        "-serial",
        "chardev:serial0",
        "-pidfile",
        "qemu.pid",
        "-daemonize",
        *extras,
    ]


def _start_process(
    state: dict[str, Any], extras: Iterable[str | int | Path]
) -> None:
    directory = Path(state["directory"])
    allocation = state["resources"]
    args = _qemu_args(state, extras)
    with (
        resource_budget.claim(
            "vm", directory, allocation["cpus"], allocation["memoryMiB"]
        ),
        (directory / "qemu.log").open("a", encoding="utf-8") as log,
    ):
        run(args, cwd=directory, stdout=log, stderr=log)
    state["pid"] = int((directory / "qemu.pid").read_text(encoding="utf-8"))
    save(Path(state["stateFile"]), state)


def launch(  # ruff: ignore[too-many-arguments] - Explicit QEMU artifact and resource inputs.
    directory: Path,
    allocation: dict[str, int],
    *,
    disk: Path,
    variables: Path,
    key: Path,
    fingerprint: str,
    code: Path,
    previous: dict[str, Any] | None = None,
    state_file: Path | None = None,
    extras: Iterable[str | int | Path] = (),
) -> dict[str, Any]:
    """Launch a UUID-owned QEMU guest.

    Returns:
        Persisted state with the guest's private SSH endpoint and PID.
    """
    directory.mkdir(parents=True, exist_ok=True)
    guest_uuid = previous["uuid"] if previous else str(uuid.uuid4())
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", previous["sshPort"] if previous else 0))
        port = reservation.getsockname()[1]
    state: dict[str, Any] = {
        "uuid": guest_uuid,
        "stateFile": str(state_file or directory / "state.json"),
        "directory": str(directory),
        "fingerprint": fingerprint,
        "sshPort": port,
        "resources": allocation,
        "status": "starting",
        "disk": str(disk),
        "firmware": str(variables),
        "firmwareCode": str(code),
        "sshKey": str(key),
    }
    _write_ssh_config(state)
    save(Path(state["stateFile"]), state)
    _start_process(state, extras)
    return state


def screenshot(state: dict[str, Any]) -> Path:
    """Save the owned guest framebuffer.

    Returns:
        The PPM path beside the guest diagnostic logs.
    """
    path = Path(state["directory"]) / "desktop.ppm"
    qmp(state, "screendump", {"filename": str(path)})
    return path


def _ssh_probe(
    state: dict[str, Any], *, installing: bool
) -> subprocess.CompletedProcess[str]:
    directory = Path(state["directory"])
    known_hosts = directory / "known_hosts"
    probe = (
        'if [ "$(findmnt -n -o FSTYPE /)" != btrfs ]; then '
        'echo "Still waiting for the installed btrfs root" >&2; exit 1; fi; '
        "test -f /etc/omarchy-release || pacman -Q omarchy"
    )
    if not installing or known_hosts.exists():
        return ssh(state, probe, check=False)
    # Pin this connection's candidate only after it proves the installed root.
    with tempfile.TemporaryDirectory(
        prefix="ssh-bootstrap-", dir=directory
    ) as temporary:
        candidate = Path(temporary) / "known_hosts"
        result = ssh(state, probe, check=False, known_hosts=candidate)
        if result.returncode == 0:
            candidate.replace(known_hosts)
        return result


def _installer_progress(state: dict[str, Any]) -> None:
    shot = screenshot(state)
    png = shot.with_suffix(".png")
    run(
        [
            "magick",
            shot,
            "-colorspace",
            "gray",
            "-negate",
            "-resize",
            "150%",
            png,
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    text = output(
        ["tesseract", png, "stdout", "--psm", "6"],
        stderr=subprocess.DEVNULL,
    ).lower()
    if "installation stopped" in text:
        message = f"Installer stopped; inspect {png}"
        raise RuntimeError(message)
    if "reboot now" in text:
        qmp(state, "send-key", {"keys": [{"type": "qcode", "data": "ret"}]})


def wait_ssh(
    state: dict[str, Any], timeout: float, *, installing: bool = False
) -> None:
    """Wait for installed-root SSH access, pinning preparation keys once.

    Raises:
        RuntimeError: If bootstrap is unsafe, the guest exits, or time expires.
    """
    deadline = time.monotonic() + timeout
    directory = Path(state["directory"])
    if installing and directory.parent != CACHE / "preparations":
        message = "SSH bootstrap is restricted to owned preparation guests"
        raise RuntimeError(message)
    last = "No SSH attempt completed"
    while time.monotonic() < deadline:
        if not running(state):
            message = (
                f"Guest exited before SSH readiness; last SSH failure: {last}"
            )
            raise RuntimeError(message)
        try:
            result = _ssh_probe(state, installing=installing)
            if result.returncode == 0:
                return
            detail = result.stderr.strip() or result.stdout.strip()
            last = f"exit {result.returncode}: {detail}"
        except subprocess.TimeoutExpired as error:
            detail = error.stderr or error.stdout or ""
            detail = (
                detail.decode(errors="replace")
                if isinstance(detail, bytes)
                else detail
            ).strip()
            last = f"SSH command timed out after {error.timeout}s: {detail}"
        if installing:
            _installer_progress(state)
        time.sleep(5)
    message = (
        f"SSH readiness timed out; last SSH failure: {last}; "
        f"inspect {directory}"
    )
    raise RuntimeError(message)


def _request_shutdown(state: dict[str, Any]) -> None:
    # The desktop may handle the ACPI button as a menu rather than poweroff.
    try:
        result = ssh(
            state,
            "printf '%s\\n' omarchy | sudo -S systemctl poweroff",
            check=False,
        )
        shutdown_requested = result.returncode == 0
    except subprocess.TimeoutExpired:
        shutdown_requested = False
    if not shutdown_requested and running(state):
        qmp(state, "system_powerdown")


def stop(state: dict[str, Any] | None, *, force: bool = True) -> None:
    """Stop the owned guest, retaining state if shutdown fails.

    Raises:
        RuntimeError: If clean shutdown is required or QEMU does not stop.
    """
    if not running(state):
        return
    _request_shutdown(state)
    deadline = time.monotonic() + 90
    while running(state) and time.monotonic() < deadline:
        time.sleep(1)
    if running(state):
        if not force:
            message = (
                "Clean shutdown failed; base not published. "
                "Use desktop-stop for owned guest."
            )
            raise RuntimeError(message)
        qmp(state, "quit")
        deadline = time.monotonic() + 10
        while running(state) and time.monotonic() < deadline:
            time.sleep(1)
    if running(state):
        message = "Owned QEMU did not stop; state retained"
        raise RuntimeError(message)
    state["status"] = "stopped"
    save(Path(state["stateFile"]), state)
