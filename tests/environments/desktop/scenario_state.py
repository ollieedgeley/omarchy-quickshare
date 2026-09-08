"""Public application and independent-peer state for retained scenarios."""  # ruff: ignore[implicit-namespace-package] - Imported by standalone scenario tooling.

import hashlib
import json
import os
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Retain bounded receiver-reset tool output.
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import deploy_native
import reference_peer
import vm_control

TERMINAL = {"completed", "failed", "rejected", "cancelled"}
PEER_TERMINAL = {"kComplete", "kCancelled", "kFailed", "kReject", "kTimedOut"}


def now() -> str:
    """Return an ISO UTC timestamp for retained evidence.

    Returns:
        Current UTC timestamp.
    """
    return datetime.now(UTC).isoformat()


def guest_command(state: dict[str, Any], command: str, attempt: Path) -> str:
    """Run pinned SSH and retain command, result and timestamps.

    Returns:
        Successful guest standard output.
    """
    started = now()
    result = vm_control.ssh(
        state, "bash -lc " + shlex.quote(command), check=False
    )
    with (attempt / "commands.jsonl").open("a", encoding="utf-8") as log:
        log.write(
            json.dumps({
                "startedAt": started,
                "finishedAt": now(),
                "command": command,
                "exitCode": result.returncode,
                "stdout": result.stdout,
                "stderr": result.stderr,
            })
            + "\n"
        )
    result.check_returncode()
    return result.stdout


def public_cli(state: dict[str, Any], attempt: Path, *args: str) -> str:
    """Invoke the installed public CLI without changing service ownership.

    Returns:
        Successful CLI standard output.
    """
    executable = (
        shlex.quote(state["cliExecutable"])
        if "cliExecutable" in state
        else '"$HOME/.local/bin/omarchy-quickshare"'
    )
    return guest_command(
        state,
        deploy_native.SESSION
        + "set -Eeuo pipefail\n"
        + f"timeout 10 {executable} "
        + shlex.join(args),
        attempt,
    )


def public_snapshot(state: dict[str, Any], attempt: Path) -> dict[str, Any]:
    """Read the versioned public snapshot directly from the owned daemon.

    Returns:
        The endpoint snapshot.

    Raises:
        RuntimeError: Public status did not return a snapshot.
    """
    if not vm_control.running(state):
        message = "Owned guest is not running"
        raise RuntimeError(message)
    daemon = state.get("daemonExecutable") or str(
        Path(
            state.get("cliExecutable", "~/.local/bin/omarchy-quickshare")
        ).with_name("omarchy-quickshare-daemon")
    )
    script = """import json, os, socket, struct, subprocess, sys
from pathlib import Path
def output(*args):
    return subprocess.check_output(args, text=True, timeout=5).strip()
def pid():
    return int(output('systemctl', '--user', 'show',
                      'omarchy-quickshare.service', '-p', 'MainPID', '--value'))
selected = Path(sys.argv[1]).expanduser()
before = pid()
loaded = Path(f'/proc/{before}/exe')
if before <= 0 or str(loaded.readlink()) != str(selected):
    raise RuntimeError('Selected installed daemon is not the loaded service')
version = int(output(str(loaded), '--protocol-version'))
if version <= 0:
    raise RuntimeError('Loaded daemon has no valid protocol identity')
path = Path(os.environ['XDG_RUNTIME_DIR']) / 'omarchy-quickshare/control.sock'
limit = 1_048_576
with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
    stream.settimeout(5)
    stream.connect(str(path))
    credentials = stream.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
    if struct.unpack('3i', credentials)[0] != before:
        raise RuntimeError('Control socket does not belong to selected daemon')
    stream.sendall(json.dumps({
        'version': version, 'request': {'type': 'snapshot'},
    }).encode() + b'\\n')
    with stream.makefile('rb') as response:
        frame = response.readline(limit + 2)
if len(frame) > limit + 1 or not frame.endswith(b'\\n'):
    raise RuntimeError('Invalid bounded public snapshot response')
result = json.loads(frame)
if result.get('version') != version or pid() != before:
    raise RuntimeError('Daemon identity changed during public snapshot')
print(json.dumps(result))
"""
    envelope = json.loads(
        guest_command(
            state,
            deploy_native.SESSION
            + shlex.join(["timeout", "20", "python", "-c", script, daemon]),
            attempt,
        )
    )
    response = envelope["response"]
    if response["type"] != "snapshot":
        message = "Public status did not return a snapshot envelope"
        raise RuntimeError(message)
    return response["snapshot"]


def peer_state(owner: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    """Verify the retained independent peer and relay identities and health.

    Returns:
        The ownership record and inspected receiver container.

    Raises:
        RuntimeError: Ownership or service health does not agree.
    """
    record = reference_peer._retained(owner)  # ruff: ignore[private-member-access] - Reuse the lifecycle owner's record validation.
    if record.get("status") != "ready":
        message = (
            "Reference peer is not ready; run desktop-peer-start explicitly"
        )
        raise RuntimeError(message)
    containers = reference_peer._owned(record)  # ruff: ignore[private-member-access] - Reuse container ownership checks without lifecycle changes.
    actual = {
        container["Config"]["Labels"]["com.docker.compose.service"]: container
        for container in containers
    }
    if set(actual) != {"peer", "relay"}:
        message = "Owned peer/relay containers are missing or ambiguous"
        raise RuntimeError(message)
    for name, container in actual.items():
        if (
            record["containers"].get(name) != container["Id"]
            or not container["State"]["Running"]
            or container["State"].get("Health", {}).get("Status") != "healthy"
        ):
            message = f"Retained {name} identity or health changed; inspect it"
            raise RuntimeError(message)
    return record, actual["peer"]


def receiver_logs(  # ruff: ignore[too-many-arguments] - Generation filtering is separate from the evidence destination.
    container: str, attempt: Path, label: str, *, since: str | None = None
) -> tuple[str, str]:
    """Retain separate receiver stdout/stderr without consuming events.

    Returns:
        Complete captured stdout and stderr.
    """
    result = vm_control.run(
        [
            os.environ.get("DOCKER", "docker"),
            "logs",
            *(["--since", since] if since else []),
            container,
        ],
        capture_output=True,
        text=True,
        timeout=15,
    )
    for name, content in (("stdout", result.stdout), ("stderr", result.stderr)):
        (attempt / f"receiver-{label}.{name}.log").write_text(
            content, encoding="utf-8"
        )
    return result.stdout, result.stderr


def parse_events(log: str) -> list[dict[str, str]]:
    """Parse the independent receiver's QS_EVENT evidence.

    Returns:
        Parsed event field dictionaries.
    """
    return [
        dict(
            field.split("=", 1)
            for field in line.removeprefix("QS_EVENT ").split()
        )
        for line in log.splitlines()
        if line.startswith("QS_EVENT ")
    ]


def _prior_owner(  # ruff: ignore[too-many-branches] - Fail closed across distinct process, share, file and retained-record identities.
    state: dict[str, Any], active: dict[str, Any], application: dict[str, Any]
) -> tuple[Path, dict[str, Any]]:
    owner = Path(state["directory"])
    matches = []
    for path in (owner / "scenarios").glob("*/result.json"):
        record = json.loads(path.read_text(encoding="utf-8"))
        submitted = record.get("submitted", {})
        source = path.parent / f"quickshare-{path.parent.name}.bin"
        previous = record.get("application", {})
        identity = {
            "attemptId": path.parent.name,
            "scenario": "file-to-reference-peer",
            "guestUuid": state["uuid"],
            "qemuPid": state["pid"],
        }
        if any(record.get(key) != value for key, value in identity.items()):
            continue
        if any(
            previous.get(key) != application[key]
            for key in ("mainPid", "loadedSha256")
        ):
            continue
        intended = (
            "shareId" not in record
            and record.get("submittedAt")
            and record.get("action") == "submit public send to selected peer"
        )
        if record.get("shareId") != active["id_string"] and not intended:
            continue
        attachment = {
            "type": "file",
            "name": source.name,
            "size_bytes": submitted.get("sizeBytes"),
        }
        if (
            active["direction"] != "outbound"
            or active["attachment"] != attachment
            or active.get("peer", {}).get("id")
            != record.get("selectedPeer", {}).get("id")
            or submitted.get("sourcePath") != str(source)
        ):
            continue
        if (
            source.is_file()
            and not source.is_symlink()
            and hashlib.sha256(source.read_bytes()).hexdigest()
            == submitted.get("sha256")
        ):
            matches.append((path, record))
    if len(matches) != 1:
        message = (
            f"Application reset refused unrelated or unproven share "
            f"{active['id_string']}; "
            f"found {len(matches)} matching attempt records"
        )
        raise RuntimeError(message)
    return matches[0]


def _settled_share(
    state: dict[str, Any], attempt: Path, active: dict[str, Any]
) -> dict[str, Any]:
    share_id = active["id_string"]
    if active["phase"] not in TERMINAL:
        public_cli(state, attempt, "share", "cancel", share_id)
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        current = public_snapshot(state, attempt).get("active_share")
        if not current or current["id_string"] != share_id:
            message = (
                f"Application reset lost owned share {share_id} "
                "before terminal evidence"
            )
            raise RuntimeError(message)
        if current["phase"] in TERMINAL:
            return current
        time.sleep(0.5)
    message = f"Application reset timed out cancelling owned share {share_id}"
    raise RuntimeError(message)


def _connections(record: dict[str, Any]) -> dict[str, Any]:
    # Read only this isolated receiver's process and TCP state.
    probe = """import ipaddress, json, pathlib, sys
guest = ipaddress.IPv4Address(sys.argv[1]).packed[::-1].hex().upper()
connections = []
for name in ('tcp', 'tcp6'):
    for line in pathlib.Path('/proc/net/' + name).read_text().splitlines()[1:]:
        fields = line.split()
        remote = fields[2].split(':')[0]
        if remote.endswith(guest) and fields[3] in {'01', '02', '03', '08'}:
            connections.append(line.strip())
expected = ['/usr/local/bin/nearby_sharing_cli', 'receive', '--name',
            sys.argv[2], '--timeout', '0', '--action', 'accept']
pids = []
for path in pathlib.Path('/proc').glob('[0-9]*/cmdline'):
    try:
        arguments = path.read_bytes().decode().rstrip(chr(0)).split(chr(0))
    except (FileNotFoundError, ProcessLookupError):
        continue
    if arguments == expected:
        pids.append(int(path.parent.name))
print(json.dumps({'guestConnections': connections, 'receiverPids': pids}))
"""
    return json.loads(
        vm_control.output(
            [
                os.environ.get("DOCKER", "docker"),
                "exec",
                record["containers"]["peer"],
                "python3",
                "-c",
                probe,
                record["guestAddress"],
                record["name"],
            ],
            timeout=15,
        )
    )


def _peer_idle(
    record: dict[str, Any], attempt: Path, label: str
) -> dict[str, Any]:
    deadline = time.monotonic() + 30
    last = {}
    while time.monotonic() < deadline:
        _, container = peer_state(Path(record["owner"]))
        stdout, _ = receiver_logs(
            container["Id"],
            attempt,
            label,
            since=container["State"]["StartedAt"],
        )
        events = parse_events(stdout)
        transfers = {}
        for event in events:
            if event.get("event") == "transfer":
                transfers[event["target_id"]] = event["status"]
        last = {
            "containerId": container["Id"],
            "startedAt": container["State"]["StartedAt"],
            "targetStates": transfers,
            **_connections(record),
        }
        # Health alone does not prove that the persistent receiver is idle.
        if (
            all(status in PEER_TERMINAL for status in transfers.values())
            and not last["guestConnections"]
            and len(last["receiverPids"]) == 1
        ):
            last["observedAt"] = now()
            return last
        time.sleep(0.5)
    message = f"Reference peer reset did not settle transfer targets: {last}"
    raise RuntimeError(message)


def _restart_peer(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> None:
    record = report["peer"]
    evidence = report["reset"]
    evidence["peerBefore"] = _peer_idle(record, attempt, "reset-before")
    evidence["generation"] = attempt.name
    vm_control.save(attempt / "reset.json", evidence)
    # The pinned Google CLI retains consent and listener state after a share.
    # Only its owned receiver is restarted; the relay and desktop stay running.
    with (attempt / "peer-reset.log").open("w", encoding="utf-8") as log:
        vm_control.run(
            [
                os.environ.get("DOCKER", "docker"),
                "restart",
                "--time",
                "10",
                record["containers"]["peer"],
            ],
            stdout=log,
            stderr=subprocess.STDOUT,
            timeout=45,
        )
    deadline = time.monotonic() + 45
    last = "Receiver did not become healthy after its reset"
    while time.monotonic() < deadline:
        try:
            _, container = peer_state(Path(record["owner"]))
            if (
                container["State"]["StartedAt"]
                != evidence["peerBefore"]["startedAt"]
            ):
                break
            last = "Receiver reset did not change its process generation"
        except RuntimeError as error:
            last = str(error)
        time.sleep(0.5)
    else:
        raise RuntimeError(last)
    evidence["receiverStartedAt"] = container["State"]["StartedAt"]
    source = (
        Path(__file__)
        .with_name("peer_discovery.py")
        .read_text(encoding="utf-8")
    )
    command = shlex.join([
        "python3",
        "-c",
        source,
        record["guestAddress"],
        record["peerAddress"],
    ])
    evidence["readiness"] = json.loads(guest_command(state, command, attempt))
    evidence["peerAfter"] = _peer_idle(record, attempt, "reset-after")
    report["receiverStartedAt"] = evidence["peerAfter"]["startedAt"]


def reset(state: dict[str, Any], attempt: Path, report: dict[str, Any]) -> None:
    """Reset owned terminal work and the receiver, retaining unrelated services.

    Continuity scenarios call their run body without this independent reset.

    Raises:
        RuntimeError: Application ownership or peer idle is unproved.
        KeyboardInterrupt: Reset was interrupted, with evidence retained.
    """
    evidence: dict[str, Any] = {
        "startedAt": now(),
        "status": "failed",
        "resource": "application",
    }
    report["reset"] = evidence
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Record every reset failure against its affected resource.
        initial = public_snapshot(state, attempt)
        evidence["applicationBefore"] = initial
        active = initial.get("active_share")
        if active:
            path, previous = _prior_owner(state, active, report["application"])
            evidence["previousAttempt"] = str(path)
            evidence["previousStatus"] = previous["status"]
            evidence["applicationTerminal"] = _settled_share(
                state, attempt, active
            )
        evidence["resource"] = "reference peer"
        _restart_peer(state, attempt, report)
        evidence["resource"] = "application"
        if active:
            public_cli(state, attempt, "share", "dismiss", active["id_string"])
        observed = public_snapshot(state, attempt)
        if observed.get("active_share") is not None:
            message = (
                "Application reset still has an active share after dismissal"
            )
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Outer handler records and re-raises the original reset error.
        evidence["applicationAfter"] = observed
        evidence["status"] = "ready"
    except (Exception, KeyboardInterrupt) as error:
        evidence["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        evidence["finishedAt"] = now()
        vm_control.save(attempt / "reset.json", evidence)
