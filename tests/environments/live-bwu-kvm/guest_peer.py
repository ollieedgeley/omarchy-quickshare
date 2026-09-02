"""Run the isolated guest side of the live Bluetooth bandwidth-upgrade test."""

import hashlib
import os
import re
import signal
import socket
import sys
import threading
import time
from importlib import import_module
from pathlib import Path
from typing import Any

PEER = sys.argv[1]
MESSAGE = b"oqs-live-bwu-reference"
REPLY = b"oqs-live-bwu-ack"
PAYLOAD = b"oqs-live-bwu-peer-payload-after-upgrade\n" * 16384
PAYLOAD_SHA256 = hashlib.sha256(PAYLOAD).hexdigest()
PEER_RESULT_TIMEOUT_SECONDS = 5
PEER_BINARY = "/opt/nearby/bin/connections_peer"
PEER_LOADER = "/opt/nearby/loader"
PEER_LIBRARY_PATH = (
    "/opt/nearby/root/lib/x86_64-linux-gnu:"
    "/opt/nearby/root/usr/lib/x86_64-linux-gnu"
)
_HCI_CONFIG = "/usr/bin/hciconfig"
_CONTROL_PORT_NAME = "oqs.control"
_CONNECTIONS_LOG = Path("/run/connections-peer.log")
_BLUETOOTHD_LOG = Path("/run/bluetoothd.log")
_CONTROLLER_ADDRESS_UNAVAILABLE = "controller address unavailable"
_CONTROL_PORT_UNAVAILABLE = "virtio control port unavailable"
_REFERENCE_BYTES_MISSING = "reference bytes did not arrive"
_REFERENCE_ACKNOWLEDGEMENT_MISSING = "reference acknowledgement did not arrive"
_HCI_BRING_UP_FAILED = "HCI bring-up failed"
_PEER_CLEANUP_INCOMPLETE = "guest peer cleanup is incomplete"
_UNKNOWN_COMMAND = "unknown command"
_SUBPROCESS_RUN_NAME = "run"
_run = getattr(import_module("subprocess"), _SUBPROCESS_RUN_NAME)


class _PeerError(RuntimeError):
    pass


def _one_hci() -> bool:
    return len(list(Path("/sys/class/bluetooth").glob("hci*"))) == 1


def _controller_address() -> str:
    result = _run(
        [_HCI_CONFIG, "hci0"], capture_output=True, text=True, check=True
    )
    match = re.search(r"BD Address: ([0-9A-F:]{17})", result.stdout)
    if not match:
        raise _PeerError(_CONTROLLER_ADDRESS_UNAVAILABLE)
    return match.group(1)


def _control_path() -> str:
    for candidate in Path("/sys/class/virtio-ports").glob("*"):
        if (candidate / "name").read_text(
            encoding="utf8"
        ).strip() == _CONTROL_PORT_NAME:
            return str(Path("/dev") / candidate.name)
    raise _PeerError(_CONTROL_PORT_UNAVAILABLE)


def _server(target: tuple[int, str, int], result: list[str]) -> None:
    family, bind_address, port = target
    listener = socket.socket(
        family,
        socket.SOCK_STREAM,
        socket.BTPROTO_RFCOMM if family == socket.AF_BLUETOOTH else 0,
    )
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((bind_address, port))
    listener.listen(1)
    connection, _ = listener.accept()
    with connection:
        if connection.recv(len(MESSAGE)) != MESSAGE:
            raise _PeerError(_REFERENCE_BYTES_MISSING)
        connection.sendall(REPLY)
    listener.close()
    result.append("OK")


def _launch_server(kind: str) -> tuple[threading.Thread, list[str]]:
    result = []
    if kind == "lan":
        target = (socket.AF_INET, socket.inet_ntoa(b"\0\0\0\0"), 47000)
    else:
        target = (socket.AF_BLUETOOTH, "00:00:00:00:00:00", 1)
    thread = threading.Thread(
        target=_server, args=(target, result), daemon=True
    )
    thread.start()
    return thread, result


def _client(kind: str, target: str) -> None:
    if kind == "lan":
        connection = socket.create_connection((target, 47000), timeout=10)
    else:
        connection = socket.socket(
            socket.AF_BLUETOOTH, socket.SOCK_STREAM, socket.BTPROTO_RFCOMM
        )
        connection.settimeout(20)
        connection.connect((target, 1))
    with connection:
        connection.sendall(MESSAGE)
        if connection.recv(len(REPLY)) != REPLY:
            raise _PeerError(_REFERENCE_ACKNOWLEDGEMENT_MISSING)


def _reply(descriptor: int, message: str) -> None:
    os.write(descriptor, f"{message}\n".encode())


def _bring_up() -> None:
    output = None
    for _ in range(50):
        output = _run(
            [_HCI_CONFIG, "hci0", "up"],
            capture_output=True,
            text=True,
            check=False,
            timeout=5,
        )
        if output.returncode == 0:
            break
        time.sleep(0.1)
    if output is None:
        raise _PeerError(_HCI_BRING_UP_FAILED)
    if output.returncode == 0:
        output = _run(
            [_HCI_CONFIG, "hci0", "piscan"],
            capture_output=True,
            text=True,
            check=False,
            timeout=5,
        )
    if output.returncode != 0:
        raise _PeerError(_HCI_BRING_UP_FAILED)


def _results(active: dict[str, Any]) -> str:
    time.sleep(0.2)
    complete = (name for name, (_, value) in active.items() if value == ["OK"])
    return ",".join(sorted(complete))


def _start_peer(active: dict[str, Any], role: str) -> None:
    payload = Path("/run/oqs-bwu-payload")
    arguments = [
        PEER_LOADER,
        "--library-path",
        PEER_LIBRARY_PATH,
        PEER_BINARY,
        f"--{role}",
        "--initial-medium=ble",
        "--upgrade-medium=bluetooth",
        "--auto-upgrade",
        f"--endpoint-name=oqs-kvm-{PEER}",
    ]
    if role == "advertise":
        payload.write_bytes(PAYLOAD)
        arguments.append(f"--send-file={payload}")
    else:
        arguments.append("--initiate-upgrade-on-connect")
    log = _CONNECTIONS_LOG.open("w", encoding="utf8")
    active["peer"] = os.posix_spawn(
        PEER_LOADER,
        arguments,
        os.environ,
        file_actions=[
            (os.POSIX_SPAWN_DUP2, log.fileno(), 1),
            (os.POSIX_SPAWN_DUP2, log.fileno(), 2),
        ],
    )
    log.close()
    _wait_for_peer_event(role)


def _wait_for_peer_event(role: str) -> None:
    deadline = time.monotonic() + 5
    event = "advertising" if role == "advertise" else "discovery"
    expected = f'"event":"{event}"'
    while time.monotonic() < deadline:
        try:
            if expected in _CONNECTIONS_LOG.read_text(encoding="utf8"):
                return
        except FileNotFoundError:
            pass
        time.sleep(0.1)
    message = f"peer {role} did not start"
    raise _PeerError(message)


def _peer_result(descriptor: int) -> None:
    deadline = time.monotonic() + PEER_RESULT_TIMEOUT_SECONDS
    events = []
    while time.monotonic() < deadline:
        try:
            evidence = _CONNECTIONS_LOG.read_text(encoding="utf8")
            events = _peer_events(evidence)
            if _peer_transfer_complete(evidence):
                _reply(descriptor, "BWU_BLE_TO_BLUETOOTH_OK")
                return
        except FileNotFoundError:
            pass
        time.sleep(0.1)
    message = "BLE to Bluetooth Classic peer evidence timed out: " + ",".join(
        events
    )
    raise _PeerError(message)


def _peer_events(evidence: str) -> list[str]:
    return re.findall(r'"event":"([^"]+)"', evidence)


def _peer_transfer_complete(evidence: str) -> bool:
    required = (
        '"initial_medium":"ble"',
        '"new_medium":"bluetooth"',
        '"payload-terminal"',
        '"status":"success"',
    )
    return all(token in evidence for token in required) and _payload_matches(
        evidence
    )


def _payload_matches(evidence: str) -> bool:
    paths = re.findall(r'"received_file":"([^"]+)"', evidence)
    if not paths:
        return PEER == "a" and f'"bytes_transferred":{len(PAYLOAD)}' in evidence
    return (
        hashlib.sha256(Path(paths[-1]).read_bytes()).hexdigest()
        == PAYLOAD_SHA256
    )


def _peer_evidence(descriptor: int) -> None:
    try:
        output = _CONNECTIONS_LOG.read_text(encoding="utf8")
        events = re.findall(r'"event":"([^"]+)"', output)
        statuses = re.findall(r'"status":"([^"]+)"', output)
    except FileNotFoundError:
        events = []
        statuses = []
    message = "PEER_EVENTS " + ",".join(events + statuses)
    message += " " + _active_process()
    message += " " + _bluez_evidence()
    _reply(descriptor, message)


def _active_process() -> str:
    try:
        output = _CONNECTIONS_LOG.read_text(encoding="utf8")
    except FileNotFoundError:
        return "not-started"
    result = "no-structured-events"
    if "error while loading shared libraries" in output:
        match = re.search(
            r"error while loading shared libraries: ([^\\n]+)", output
        )
        if match:
            result = "loader-error-" + match.group(1)[:120]
        else:
            result = "loader-error"
    elif "Failed to connect to system bus" in output:
        result = "system-bus-error"
    return result


def _bluez_evidence() -> str:
    try:
        output = _BLUETOOTHD_LOG.read_text(encoding="utf8")
    except FileNotFoundError:
        return "bluez-log-missing"
    evidence = "bluez-no-matching-error"
    for token, result in (
        ("Failed to start discovery", "bluez-discovery-start-failed"),
        ("No default controller available", "bluez-no-controller"),
        ("Failed to set scan parameters", "bluez-scan-parameters-failed"),
    ):
        if token in output:
            evidence = result
            break
    return evidence


def _stop_peer(active: dict[str, Any]) -> None:
    process_id = active.pop("peer", None)
    if process_id is not None:
        os.kill(process_id, signal.SIGTERM)
        deadline = time.monotonic() + 1
        try:
            while time.monotonic() < deadline:
                if os.waitpid(process_id, os.WNOHANG)[0]:
                    return
                time.sleep(0.1)
            os.kill(process_id, signal.SIGKILL)
            os.waitpid(process_id, 0)
        except ChildProcessError:
            return


def _peer_clean(active: dict[str, Any], descriptor: int) -> None:
    if "peer" in active or not _one_hci():
        raise _PeerError(_PEER_CLEANUP_INCOMPLETE)
    _reply(descriptor, "PEER_CLEAN")


def _handle(
    command: list[str], active: dict[str, Any], descriptor: int
) -> bool:
    if command == ["IDENTITY"]:
        _identity(descriptor)
    elif command in (["LAN_LISTEN"], ["CLASSIC_LISTEN"]):
        kind = "lan" if command[0] == "LAN_LISTEN" else "classic"
        active[kind] = _launch_server(kind)
        _reply(descriptor, f"{kind.upper()}_READY")
    elif command and command[0] in {"LAN_SEND", "CLASSIC_SEND"}:
        kind = "lan" if command[0] == "LAN_SEND" else "classic"
        _client(kind, command[1])
        _reply(descriptor, f"{kind.upper()}_BYTES_OK")
    elif command == ["STOP"]:
        _reply(descriptor, "STOPPING")
        return False
    else:
        _handle_peer_command(command, active, descriptor)
    _reply(descriptor, "STATUS 0")
    return True


def _identity(descriptor: int) -> None:
    if not _one_hci():
        message = "guest owns more than one controller"
        raise _PeerError(message)
    _reply(descriptor, "HCI_COUNT 1")
    _reply(descriptor, f"ADDRESS {_controller_address()}")


def _handle_peer_command(
    command: list[str], active: dict[str, Any], descriptor: int
) -> None:
    name = command[0] if len(command) == 1 else ""
    handlers = {
        "RESULTS": lambda: _reply(descriptor, "RESULTS " + _results(active)),
        "BRING_UP": _bring_up,
        "PEER_ADVERTISE": lambda: _start_peer(active, "advertise"),
        "PEER_DISCOVER": lambda: _start_peer(active, "discover"),
        "PEER_RESULT": lambda: _peer_result(descriptor),
        "PEER_EVIDENCE": lambda: _peer_evidence(descriptor),
        "PEER_STOP": lambda: _stop_peer(active),
        "PEER_CLEAN": lambda: _peer_clean(active, descriptor),
    }
    handler = handlers.get(name)
    if handler is None:
        raise _PeerError(_UNKNOWN_COMMAND)
    handler()


def _control_session(active: dict[str, Any]) -> bool:
    descriptor = os.open(_control_path(), os.O_RDWR)
    with os.fdopen(descriptor, "rb") as control:
        _reply(descriptor, "READY")
        for raw in control:
            try:
                if not _handle(
                    raw.decode().strip().split(), active, descriptor
                ):
                    return False
            except (
                OSError,
                RuntimeError,
                UnicodeDecodeError,
                ValueError,
            ) as error:
                _reply(descriptor, f"ERROR {error}\\nSTATUS 1")
    return True


def _main() -> None:
    active = {}
    while _control_session(active):
        pass


if __name__ == "__main__":
    _main()
