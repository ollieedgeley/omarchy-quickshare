"""Guest-side component installation and socket readiness over SSH."""  # ruff: ignore[implicit-namespace-package] - Copied standalone guest program.

import hashlib
import json
import os
import shutil
import socket
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Fixed guest command arguments.
import sys
import tempfile
from pathlib import Path

FILES = {"cli": "omarchy-quickshare", "daemon": "omarchy-quickshare-daemon"}
UNIT = "omarchy-quickshare.service"
MAX_HEALTH_FRAME = 65536


def _output(*args: str) -> str:
    return subprocess.check_output(args, text=True, timeout=10).strip()  # ruff: ignore[subprocess-without-shell-equals-true] - Fixed argv, never shell input.


def _digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def _protocol(path: Path, role: str) -> int:
    flag = "protocol-version" if role == "cli" else "--protocol-version"
    return int(_output(str(path), flag))


def _runtime() -> dict:
    instances = json.loads(_output("hyprctl", "instances", "-j"))
    if not instances:
        message = "No retained guest compositor"
        raise RuntimeError(message)
    shells = _output("pgrep", "-u", str(os.getuid()), "-x", "quickshell|qs")
    pids = sorted(
        {int(item["pid"]) for item in instances}
        | {int(pid) for pid in shells.splitlines()}
    )
    return {
        str(pid): Path(f"/proc/{pid}/stat")
        .read_text(encoding="utf-8")
        .rsplit(")", 1)[1]
        .split()[19]
        for pid in pids
    }


def _service() -> dict:
    properties = dict(
        line.split("=", 1)
        for line in _output(
            "systemctl",
            "--user",
            "show",
            UNIT,
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "MainPID",
        ).splitlines()
    )
    state = {
        "loadState": properties["LoadState"],
        "activeState": properties["ActiveState"],
        "mainPid": int(properties["MainPID"]),
    }
    if (
        state["loadState"] not in {"loaded", "not-found"}
        or state["activeState"] not in {"active", "inactive"}
        or (state["activeState"] == "active") != (state["mainPid"] > 0)
        or (
            state["loadState"] == "not-found"
            and state["activeState"] != "inactive"
        )
    ):
        message = f"Unsupported daemon service state: {state}"
        raise RuntimeError(message)
    return state


def _identity(version: int | None = None) -> dict:
    service = _service()
    records = {}
    for role, name in FILES.items():
        path = Path.home() / ".local/bin" / name
        if path.exists():
            records[role] = {"file": name, "installedSha256": _digest(path)}
    records["daemon"] = {**records.get("daemon", {}), **service}
    unit = Path.home() / ".config/systemd/user" / UNIT
    if unit.exists():
        records["daemon"]["unitSha256"] = _digest(unit)
    if service["activeState"] == "active":
        executable = Path(f"/proc/{service['mainPid']}/exe")
        loaded = str(executable.readlink())
        role = (
            "cli"
            if Path(loaded.removesuffix(" (deleted)")).name == FILES["cli"]
            else "daemon"
        )
        observed_version = _protocol(executable, role)
        if observed_version <= 0 or (
            version is not None and observed_version != version
        ):
            message = (
                f"Live daemon protocol {observed_version} "
                f"differs from expected {version}"
            )
            raise RuntimeError(message)
        records["daemon"].update({
            "loadedSha256": _digest(executable),
            "executable": loaded,
            "controlProtocol": observed_version,
        })
    if _service() != service:
        message = "Daemon identity changed while reading it"
        raise RuntimeError(message)
    return {"components": records, "runtime": _runtime()}


def _health(version: int) -> None:
    path = (
        Path(os.environ["XDG_RUNTIME_DIR"]) / "omarchy-quickshare/control.sock"
    )
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
        stream.settimeout(5)
        stream.connect(str(path))
        stream.sendall(
            json.dumps({
                "version": version,
                "request": {"type": "status"},
            }).encode()
            + b"\n"
        )
        with stream.makefile("rb") as response:
            frame = response.readline(MAX_HEALTH_FRAME + 1)
    if len(frame) > MAX_HEALTH_FRAME or not frame.endswith(b"\n"):
        message = "Invalid bounded control health response"
        raise RuntimeError(message)
    result = json.loads(frame)
    if result != {"version": version, "response": {"type": "ready"}}:
        message = f"Incompatible or unavailable daemon: {result}"
        raise RuntimeError(message)


def _inputs(stage: Path, metadata: dict, roles: list[str]) -> list[tuple]:
    inputs = [
        (
            stage / FILES[role],
            Path.home() / ".local/bin" / FILES[role],
            metadata["components"][role]["sha256"],
            0o755,
            role,
        )
        for role in roles
    ]
    if "daemon" in roles:
        inputs.append((
            stage / UNIT,
            Path.home() / ".config/systemd/user" / UNIT,
            metadata["unitSha256"],
            0o644,
            None,
        ))
    return inputs


def _stage(source: Path, target: Path, mode: int) -> Path:
    target.parent.mkdir(parents=True, exist_ok=True)
    descriptor, name = tempfile.mkstemp(
        prefix=target.name + ".", dir=target.parent
    )
    os.close(descriptor)
    temporary = Path(name)
    try:
        shutil.copyfile(source, temporary)
        temporary.chmod(mode)
    except BaseException:
        temporary.unlink(missing_ok=True)
        raise
    return temporary


def _check(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Validate staged bytes and protocol together.
    temporary: Path, expected: str, role: str | None, version: int
) -> None:
    if _digest(temporary) != expected:
        message = f"Staged artifact changed: {temporary.name}"
        raise RuntimeError(message)
    if role and _protocol(temporary, role) != version:
        message = f"Staged component protocol mismatch: {role}"
        raise RuntimeError(message)


def _install(stage: Path, metadata: dict, roles: list[str]) -> None:
    pending = []
    try:
        for source, target, expected, mode, role in _inputs(
            stage, metadata, roles
        ):
            temporary = _stage(source, target, mode)
            pending.append((temporary, target))
            _check(temporary, expected, role, metadata["controlProtocol"])
        for temporary, target in pending:
            temporary.replace(target)
        if "daemon" in roles:
            _output("systemctl", "--user", "daemon-reload")
            _output("systemctl", "--user", "restart", UNIT)
    finally:
        for temporary, _ in pending:
            temporary.unlink(missing_ok=True)


def main() -> None:
    """Run the selected operation in the existing guest user session.

    Raises:
        RuntimeError: The daemon changes during its health probe.
    """
    mode = sys.argv[1]
    if mode == "install":
        _install(
            Path(sys.argv[2]), json.loads(sys.argv[3]), json.loads(sys.argv[4])
        )
        return
    result = _identity(int(sys.argv[2]) if sys.argv[2:] else None)
    daemon = result["components"]["daemon"]
    if daemon["activeState"] == "active":
        _health(daemon["controlProtocol"])
    if _service() != {
        key: daemon[key] for key in ("loadState", "activeState", "mainPid")
    }:
        message = "Daemon identity changed during its health probe"
        raise RuntimeError(message)
    if mode == "probe" and daemon["activeState"] != "active":
        message = "Daemon is not active"
        raise RuntimeError(message)
    if mode == "probe":
        print("available")
        print(daemon["loadedSha256"])
        print(daemon["executable"])
        print(daemon["mainPid"])
    else:
        print(json.dumps(result))


if __name__ == "__main__":
    main()
