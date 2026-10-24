"""Send one synthetic file through the installed CLI to the retained peer."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import fcntl
import hashlib
import json
import operator
import os
import re
import shlex
import stat
import sys
import time
import uuid
from decimal import Decimal
from pathlib import Path
from typing import Any

import build_native
import deploy_native
import lab_lifecycle
import resource_budget
import scenario_fault
import vm_control
from scenario_state import (
    TERMINAL,
    guest_command,
    now,
    parse_events,
    peer_state,
    public_cli,
    public_snapshot,
    receiver_logs,
    reset,
)

COMPLETE_PERCENT = Decimal(100)


def _application(state: dict[str, Any], attempt: Path) -> dict[str, Any]:
    health, checksum, executable, pid = (
        guest_command(state, deploy_native.PROBE, attempt).strip().splitlines()
    )
    loaded = checksum.split()[0]
    installed = guest_command(
        state, 'sha256sum "$HOME/.local/bin/omarchy-quickshare-daemon"', attempt
    ).split()[0]
    if health != "available" or installed != loaded:
        message = "Installed daemon and running service identity disagree"
        raise RuntimeError(message)
    owner = Path(state["directory"])
    deployments = []
    for path in (owner / "deployments").glob("*/deployment.json"):
        record = json.loads(path.read_text(encoding="utf-8"))
        build = record.get("build", {})
        if (
            record.get("status") == "ready"
            and record.get("guestUuid") == state["uuid"]
            and record.get("loadedSha256") == loaded
            and (build.get("status"), build.get("schemaVersion"))
            == ("built", build_native.SCHEMA_VERSION)
            and build.get("components", {}).get("daemon", {}).get("sha256")
            == loaded
        ):
            deployments.append((record["finishedAt"], path, record))
    if not deployments:
        message = "Running binary has no successful deployment/source record"
        raise RuntimeError(message)
    _, path, deployment = max(deployments, key=operator.itemgetter(0))
    return {
        "health": health,
        "mainPid": int(pid),
        "executable": executable,
        "installedSha256": installed,
        "loadedSha256": loaded,
        "deploymentRecord": str(path),
        "deployment": deployment,
    }


def _receiver(
    container: str, attempt: Path, baseline: tuple[str, str]
) -> list[dict[str, str]]:
    current = receiver_logs(container, attempt, "latest")
    if any(
        not new.startswith(old)
        for old, new in zip(baseline, current, strict=True)
    ):
        message = (
            "Receiver logs rotated or changed; cannot correlate this attempt"
        )
        raise RuntimeError(message)
    delta = current[0][len(baseline[0]) :]
    (attempt / "receiver-attempt.log").write_text(delta, encoding="utf-8")
    events = parse_events(delta)
    vm_control.save(attempt / "receiver-events.json", {"events": events})
    return events


def _discover(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> dict[str, Any]:
    name = report["peer"]["name"]
    readiness = report["reset"]["readiness"]
    # network/worker.rs exposes MdnsInstance::label() as the LAN peer ID.
    # The fresh private-IP probe identifies this receiver generation even
    # while old same-name advertisements remain in the public peer cache.
    peer_id, separator, service = readiness["instance"].partition(".")
    if (
        not separator
        or service != readiness["service"]
        or readiness["address"] != report["peer"]["peerAddress"]
    ):
        message = "Fresh receiver DNS identity does not match its owned route"
        raise RuntimeError(message)
    initial = public_snapshot(state, attempt)
    active = initial.get("active_share")
    if active:
        message = (
            f"Public share {active['id_string']} is {active['phase']}; "
            "preserve it and explicitly resolve/dismiss it before this scenario"
        )
        raise RuntimeError(message)
    public_cli(state, attempt, "discover", "start")
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        snapshot = public_snapshot(state, attempt)
        matches = [
            peer for peer in snapshot.get("peers", []) if peer["id"] == peer_id
        ]
        if len(matches) > 1:
            message = f"Public discovery duplicated receiver ID {peer_id}"
            raise RuntimeError(message)
        if matches:
            if matches[0]["name"] != name:
                message = f"Receiver {peer_id} advertised an unexpected name"
                raise RuntimeError(message)
            return matches[0]
        time.sleep(0.5)
    message = (
        f"Public discovery did not observe receiver {name}/{peer_id} within 30s"
    )
    raise RuntimeError(message)


def _payload(state: dict[str, Any], attempt: Path) -> tuple[Path, str]:
    payload = attempt / f"quickshare-{attempt.name}.bin"
    payload.write_bytes(
        f"Synthetic retained-lab transfer {attempt.name}\n".encode()
        + bytes(range(256)) * 256
    )
    stage = f".cache/omarchy-quickshare-scenarios/{attempt.name}"
    guest_command(
        state, "umask 077; mkdir -p -- " + shlex.quote(stage), attempt
    )
    result = vm_control.run(
        [
            "scp",
            "-F",
            Path(state["directory"]) / "ssh.config",
            payload,
            f"desktop:{stage}/{payload.name}",
        ],
        capture_output=True,
        text=True,
        timeout=30,
    )
    (attempt / "copy.log").write_text(
        result.stdout + result.stderr, encoding="utf-8"
    )
    guest_path = f"{stage}/{payload.name}"
    expected = hashlib.sha256(payload.read_bytes()).hexdigest()
    actual = guest_command(
        state, "sha256sum -- " + shlex.quote(guest_path), attempt
    ).split()[0]
    if actual != expected:
        message = "Staged synthetic payload differs from retained source"
        raise RuntimeError(message)
    return payload, guest_path


def _completion(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - One bounded observation loop shares immutable attempt context.
    state: dict[str, Any],
    attempt: Path,
    report: dict[str, Any],
    baseline: tuple[str, str],
    received: Path,
) -> None:
    deadline = time.monotonic() + 60
    container = report["peer"]["containers"]["peer"]
    while time.monotonic() < deadline:
        share = public_snapshot(state, attempt).get("active_share")
        report["publicOutcome"] = share
        _check_share(share, report)
        events = _receiver(container, attempt, baseline)
        report["receiverEvents"] = events
        if share["phase"] in TERMINAL - {"completed"}:
            message = (
                f"Public transfer ended {share['phase']}: "
                f"{share.get('terminal_reason')}"
            )
            raise RuntimeError(message)
        _check_targets(events)
        completed = [
            event
            for event in events
            if event.get("event") == "transfer"
            and event.get("status") == "kComplete"
        ]
        if share["phase"] == "completed" and completed and received.exists():
            final = completed[-1]
            _check_completion(report, final)
            _record_received(attempt, report, (received, final))
            return
        time.sleep(0.5)
    message = (
        "Timed out awaiting both public completion and "
        "independent receiver completion/file"
    )
    raise RuntimeError(message)


def _check_share(share: dict[str, Any] | None, report: dict[str, Any]) -> None:
    share_id = report["shareId"]
    if not share or share["id_string"] != share_id:
        message = f"Public share {share_id} disappeared or was replaced"
        raise RuntimeError(message)
    expected = report["submitted"]
    if (
        share["attachment"]
        != {
            "type": "file",
            "name": expected["name"],
            "size_bytes": expected["sizeBytes"],
        }
        or share["direction"] != "outbound"
        or share.get("peer", {}).get("id") != report["selectedPeer"]["id"]
        or share["total_bytes"] != expected["sizeBytes"]
    ):
        message = "Public share identity or offered metadata changed"
        raise RuntimeError(message)


def _check_targets(events: list[dict[str, str]]) -> None:
    targets = {event["target_id"] for event in events if "target_id" in event}
    if len(targets) > 1:
        message = (
            "Multiple receiver targets appeared; attempt evidence is ambiguous"
        )
        raise RuntimeError(message)


def _check_completion(report: dict[str, Any], final: dict[str, str]) -> None:
    share = report["publicOutcome"]
    events = report["receiverEvents"]
    expected = report["submitted"]
    if not (
        any(event.get("event") == "incoming-offer" for event in events)
        and any(
            event.get("event") == "action-requested"
            and event.get("action") == "accept"
            for event in events
        )
        and any(
            event.get("event") == "action-result"
            and event.get("action") == "accept"
            and event.get("status") == "kOk"
            for event in events
        )
        and Decimal(final["progress"]) == COMPLETE_PERCENT
        and int(final["total_bytes"]) == expected["sizeBytes"]
        and share["transferred_bytes"] == expected["sizeBytes"]
        and share.get("medium") == "wifi_lan"
    ):
        message = (
            "Receiver consent/completion or public "
            "byte/medium evidence disagrees"
        )
        raise RuntimeError(message)


def _record_received(
    attempt: Path,
    report: dict[str, Any],
    receipt: tuple[Path, dict[str, str]],
) -> None:
    received, final = receipt
    expected = report["submitted"]
    if not stat.S_ISREG(received.lstat().st_mode):
        message = "Received path is not a regular file"
        raise RuntimeError(message)
    actual = received.read_bytes()
    report["received"] = {
        "path": str(received),
        "name": received.name,
        "sizeBytes": len(actual),
        "sha256": hashlib.sha256(actual).hexdigest(),
        "targetId": final["target_id"],
        "observedAt": now(),
    }
    if (
        actual != (attempt / expected["name"]).read_bytes()
        or report["received"]["sha256"] != expected["sha256"]
        or len(actual) != expected["sizeBytes"]
        or received.name != expected["name"]
    ):
        message = (
            "Independent receiver filename, bytes, size or SHA-256 differs"
        )
        raise RuntimeError(message)


def _prepare(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> None:
    report.update({"guestUuid": state["uuid"], "qemuPid": state["pid"]})
    report["application"] = _application(state, attempt)
    peer, container = peer_state(Path(state["directory"]))
    report["peer"] = peer
    report["receiverStartedAt"] = container["State"]["StartedAt"]
    report["action"] = "reset owned application and independent peer state"
    reset(state, attempt, report)


def _exercise(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> None:
    owner = Path(state["directory"])
    peer = report["peer"]
    report["action"] = "discover named reference through public CLI"
    report["selectedPeer"] = _discover(state, attempt, report)
    report["action"] = "stage unique synthetic file"
    payload, guest_path = _payload(state, attempt)
    report["submitted"] = {
        "name": payload.name,
        "sizeBytes": payload.stat().st_size,
        "sha256": hashlib.sha256(payload.read_bytes()).hexdigest(),
        "sourcePath": str(payload),
        "guestPath": guest_path,
    }
    received = owner / "peer-cases" / "received" / payload.name
    if received.exists() or received.is_symlink():
        message = (
            "Unique receiver destination already exists; "
            "refusing stale evidence"
        )
        raise RuntimeError(message)
    baseline = receiver_logs(peer["containers"]["peer"], attempt, "before")
    report["action"] = "apply optional controlled peer fault"
    with scenario_fault.scoped(state, attempt, report):
        _submit(state, attempt, report)
        report["action"] = "observe public and independent receiver completion"
        _completion(state, attempt, report, baseline, received)
    report["action"] = "verify retained services remain unchanged"
    application = _application(state, attempt)
    _, current_peer = peer_state(owner)
    if (
        application["mainPid"] != report["application"]["mainPid"]
        or application["loadedSha256"] != report["application"]["loadedSha256"]
        or current_peer["State"]["StartedAt"] != report["receiverStartedAt"]
    ):
        message = (
            "Application or reference receiver restarted during the attempt"
        )
        raise RuntimeError(message)


def _submit(
    state: dict[str, Any],
    attempt: Path,
    report: dict[str, Any],
) -> None:
    report["action"] = "submit public send to selected peer"
    report["submittedAt"] = now()
    # Persist send intent before the CLI call, then its assigned ID.
    vm_control.save(attempt / "result.json", report)
    output = public_cli(
        state,
        attempt,
        "send",
        "--peer",
        report["selectedPeer"]["id"],
        report["submitted"]["guestPath"],
    )
    match = re.fullmatch(
        r"Share ([0-9]+) queued\.\nrequest_id=([^\r\n]+)\s*", output
    )
    if not match:
        message = "Public send did not report its queued share identifier"
        raise RuntimeError(message)
    report["shareId"] = match[1]
    report["requestId"] = match[2]
    vm_control.save(attempt / "result.json", report)


def _attempt(owner: Path) -> None:
    fault_mode = scenario_fault.selected()
    attempt = owner / "scenarios" / str(uuid.uuid4())
    attempt.mkdir(parents=True, mode=0o700)
    report: dict[str, Any] = {
        "attemptId": attempt.name,
        "scenario": "file-to-reference-peer",
        "lab": owner.name,
        "startedAt": now(),
        "status": "failed",
        "faultMode": fault_mode,
        "scenarioSha256": hashlib.sha256(
            Path(__file__).read_bytes()
        ).hexdigest(),
        "stateSourceSha256": hashlib.sha256(
            Path(__file__).with_name("scenario_state.py").read_bytes()
        ).hexdigest(),
        "faultSourceSha256": hashlib.sha256(
            Path(scenario_fault.__file__).read_bytes()
        ).hexdigest(),
        "action": "check retained identities",
    }
    state = None
    try:
        state = deploy_native._running(owner)  # ruff: ignore[private-member-access] - Reuse the deployment owner's retained-VM check.
        with resource_budget.scenario_claim(owner):
            _prepare(state, attempt, report)
            _exercise(state, attempt, report)
        report["status"] = "passed"
    except (Exception, KeyboardInterrupt) as error:
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        report["finishedAt"] = now()
        report["labRunningAtFinish"] = bool(state and vm_control.running(state))
        try:
            _, retained_peer = peer_state(owner)
            report["peerRunningAtFinish"] = True
            report["receiverStartedAtFinish"] = retained_peer["State"][
                "StartedAt"
            ]
        except Exception as error:  # ruff: ignore[blind-except] - Diagnostics must not hide the original failure.
            report["peerRunningAtFinish"] = False
            report["peerFinishError"] = str(error)
        vm_control.save(attempt / "result.json", report)
        print(f"Scenario {report['status']}; retained evidence: {attempt}")
    print(json.dumps(report, indent=2))


def main() -> None:
    """Run only against the selected retained owner under its operation lock."""
    os.umask(0o077)
    owner = lab_lifecycle.directory()
    with (owner / "operation.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        _attempt(owner)


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:  # ruff: ignore[blind-except] - Standalone boundary reports errors with retained evidence.
        print(
            f"Scenario failed: {error}. Inspect retained lab evidence.",
            file=sys.stderr,
        )
        sys.exit(1)
