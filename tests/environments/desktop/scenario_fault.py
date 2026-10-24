"""One reversible route fault, scoped to the retained receiver's private IP."""  # ruff: ignore[implicit-namespace-package] - Imported by standalone scenario tooling.

import json
import os
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Redact transport exceptions carrying the privileged command.
import sys
from collections.abc import Iterator
from contextlib import contextmanager
from ipaddress import IPv4Network
from pathlib import Path
from typing import Any

import vm_control
from scenario_state import guest_command, now

METRIC = 42708


def selected() -> str:
    """Read the one supported explicit scenario fault.

    Returns:
        Empty string for normal execution, or peer-unreachable.

    Raises:
        RuntimeError: An unknown fault was requested.
    """
    value = os.environ.get("DESKTOP_SCENARIO_FAULT", "")
    if value not in {"", "peer-unreachable"}:
        message = f"Unknown DESKTOP_SCENARIO_FAULT: {value!r}"
        raise RuntimeError(message)
    return value


def _routes(
    state: dict[str, Any], attempt: Path, destination: str
) -> list[dict[str, Any]]:
    return json.loads(
        guest_command(
            state,
            shlex.join([
                "ip",
                "-j",
                "route",
                "show",
                "table",
                "all",
                "exact",
                destination,
            ]),
            attempt,
        )
    )


def _owned(routes: list[dict[str, Any]], destination: str) -> bool:
    if len(routes) != 1:
        return False
    route = routes[0]
    return (
        route.get("type") == "unreachable"
        and IPv4Network(route["dst"]) == IPv4Network(destination)
        and route.get("protocol") == "static"
        and route.get("metric") == METRIC
        and route.get("table", "main") == "main"
    )


def _command(action: str, destination: str) -> list[str]:
    return [
        "ip",
        "route",
        action,
        "unreachable",
        destination,
        "table",
        "main",
        "proto",
        "static",
        "metric",
        str(METRIC),
    ]


def _privileged(state: dict[str, Any], argv: list[str], attempt: Path) -> None:
    entry: dict[str, Any] = {"argv": argv, "startedAt": now()}
    # This password belongs only to the disposable lab's existing setup.
    # Keep the shell and its authentication out of evidence and exceptions.
    command = "printf '%s\\n' omarchy | sudo -S -p '' " + shlex.join(argv)
    try:
        result = vm_control.ssh(state, command, check=False)
        entry.update({
            "exitCode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        })
    except subprocess.SubprocessError as error:
        entry["transportError"] = type(error).__name__
        message = "Scoped route SSH failed; see fault-commands.jsonl"
        raise RuntimeError(message) from None
    finally:
        entry["finishedAt"] = now()
        with (attempt / "fault-commands.jsonl").open(
            "a", encoding="utf-8"
        ) as log:
            log.write(json.dumps(entry) + "\n")
    if result.returncode:
        message = (
            f"Scoped route command exited {result.returncode}; "
            "see fault-commands.jsonl"
        )
        raise RuntimeError(message)


def _restore(
    state: dict[str, Any], attempt: Path, evidence: dict[str, Any]
) -> None:
    destination = evidence["destination"]
    current = _routes(state, attempt, destination)
    evidence["cleanupBefore"] = current
    if current:
        if not _owned(current, destination):
            message = (
                f"Fault cleanup refused changed/unowned route to {destination}"
            )
            raise RuntimeError(message)
        evidence["cleanupCommand"] = _command("del", destination)
        _privileged(state, evidence["cleanupCommand"], attempt)
    observed = _routes(state, attempt, destination)
    evidence["cleanupAfter"] = observed
    if observed:
        message = f"Fault cleanup still observes a host route to {destination}"
        raise RuntimeError(message)
    evidence["restoredAt"] = now()
    evidence["restored"] = True


def _prepare_fault(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> dict[str, Any]:
    destination = str(IPv4Network(report["peer"]["peerAddress"]))
    evidence: dict[str, Any] = {
        "kind": report["faultMode"],
        "startedAt": now(),
        "guestUuid": state["uuid"],
        "qemuPid": state["pid"],
        "peerContainerId": report["peer"]["containers"]["peer"],
        "peerRuntime": report["peer"]["runtime"],
        "destination": destination,
        "table": "main",
        "type": "unreachable",
        "protocol": "static",
        "metric": METRIC,
        "restored": False,
    }
    report["fault"] = evidence
    before = _routes(state, attempt, destination)
    evidence["before"] = before
    if before:
        vm_control.save(attempt / "fault.json", evidence)
        message = f"Fault refused existing exact host route to {destination}"
        raise RuntimeError(message)
    evidence["installCommand"] = _command("add", destination)
    vm_control.save(attempt / "fault.json", evidence)
    return evidence


@contextmanager
def scoped(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> Iterator[None]:
    """Apply the selected fault around actual send/observation, then remove it.

    Yields:
        Control while the optional peer-only route fault is active.

    Raises:
        RuntimeError: Route ownership, installation or restoration is unproved.
    """
    if not report["faultMode"]:
        yield
        return
    evidence = _prepare_fault(state, attempt, report)
    destination = evidence["destination"]
    try:
        _privileged(state, evidence["installCommand"], attempt)
        evidence["installed"] = _routes(state, attempt, destination)
        if not _owned(evidence["installed"], destination):
            message = (
                "Fault did not install the exact owned unreachable host route"
            )
            raise RuntimeError(message)
        evidence["installedAt"] = now()
        vm_control.save(attempt / "fault.json", evidence)
        yield
    finally:
        original_failure = sys.exc_info()[0] is not None
        evidence["nativeOutcome"] = report.get("publicOutcome")
        try:
            _restore(state, attempt, evidence)
        except Exception as error:
            evidence["cleanupError"] = f"{type(error).__name__}: {error}"
            if not original_failure:
                raise
            print(f"Fault cleanup failed: {error}", file=sys.stderr)
        finally:
            evidence["finishedAt"] = now()
            vm_control.save(attempt / "fault.json", evidence)
