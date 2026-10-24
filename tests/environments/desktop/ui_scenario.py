"""Share a file through the installed visible guest panel."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import argparse
import fcntl
import hashlib
import json
import os
import shlex
import sys
import time
import uuid
from pathlib import Path
from typing import Any

import clean_install
import deploy_native
import lab_lifecycle
import resource_budget
import scenario
import vm_control
from guest_file_selection import copy_payload
from guest_ui import GuestUI, desktop_session, normalized
from installation_attestation import reattest
from installed_ui_control import InstalledUI, installed_cli_unavailable
from scenario_state import (
    guest_command,
    now,
    peer_state,
    public_snapshot,
    receiver_logs,
    reset,
)


def application(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Installed identity and retained stage label are separate inputs.
    state: dict[str, Any],
    installation: dict[str, Any],
    attempt: Path,
    label: str,
) -> dict[str, Any]:
    """Probe installed bytes and fresh live identities.

    Returns:
        Installed native and plugin identity evidence.
    """
    native = clean_install.probe(
        state, installation, attempt, label + "-native", "native"
    )
    plugin = clean_install.probe(
        state,
        installation,
        attempt,
        label + "-plugin",
        "plugin",
        str(uuid.uuid4()),
    )
    return {
        **native,
        "loadedSha256": native["hashes"]["loaded"],
        "plugin": plugin,
    }


def discover(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> dict[str, Any]:
    """Observe discovery started by panel opening.

    Returns:
        The uniquely named peer matching the fresh DNS identity.

    Raises:
        RuntimeError: Identity is ambiguous, missing or a share already exists.
    """
    ready = report["reset"]["readiness"]
    peer_id, separator, service = ready["instance"].partition(".")
    if (
        not separator
        or service != ready["service"]
        or ready["address"] != report["peer"]["peerAddress"]
    ):
        message = "Fresh receiver DNS identity differs from the owned route"
        raise RuntimeError(message)
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        snapshot = public_snapshot(state, attempt)
        if snapshot.get("active_share"):
            message = "Unexpected share before visible file selection"
            raise RuntimeError(message)
        peers = snapshot.get("peers", [])
        matches = [peer for peer in peers if peer["id"] == peer_id]
        names = [
            peer for peer in peers if peer["name"] == report["peer"]["name"]
        ]
        if len(matches) > 1 or len(names) > 1:
            message = (
                "Visible receiver name or public DNS identity is ambiguous"
            )
            raise RuntimeError(message)
        if matches and matches[0]["name"] == report["peer"]["name"]:
            return matches[0]
        time.sleep(0.5)
    message = "Panel discovery did not expose the freshly identified receiver"
    raise RuntimeError(message)


def unique_peer_prefix(
    snapshot: dict[str, Any], selected: dict[str, Any], prefix: str
) -> None:
    """Require the visible name prefix to identify only the selected DNS peer.

    Raises:
        RuntimeError: The peer disappeared, changed or has an ambiguous prefix.
    """
    matches = [
        peer
        for peer in snapshot.get("peers", [])
        if normalized(peer["name"]).startswith(normalized(prefix))
    ]
    if (
        len(matches) != 1
        or matches[0]["id"] != selected["id"]
        or matches[0]["name"] != selected["name"]
    ):
        message = "Visible peer prefix does not uniquely identify selected peer"
        raise RuntimeError(message)


def exercise(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Keep ordered input and its identity assertions together; no transfer step may be retried independently.
    state: dict[str, Any],
    installation: dict[str, Any],
    attempt: Path,
    report: dict[str, Any],
    ui: GuestUI,
    *,
    cli_unavailable: bool = False,
) -> None:
    """Drive file-manager copy and panel peer selection through QMP.

    Raises:
        RuntimeError: Visible input, share identity or completion disagrees.
    """
    owner = Path(state["directory"])
    report["guestHome"] = installation["absence"]["home"]
    report["application"] = application(state, installation, attempt, "before")
    report["peer"], _ = peer_state(owner)
    # Independent-case reset is the only reset. Preserve all state after input.
    reset(state, attempt, report)
    payload, guest_path = scenario._payload(state, attempt)  # ruff: ignore[private-member-access] - Reuse the scenario's private synthetic payload and staging contract.
    report["submitted"] = {
        "name": payload.name,
        "sizeBytes": payload.stat().st_size,
        "sha256": hashlib.sha256(payload.read_bytes()).hexdigest(),
        "sourcePath": str(payload),
        "guestPath": guest_path,
    }
    received = owner / "peer-cases" / "received" / payload.name
    if received.exists() or received.is_symlink():
        message = "Unique receiver destination already exists"
        raise RuntimeError(message)
    with installed_cli_unavailable(
        state, attempt, report, ui, enabled=cli_unavailable
    ):
        _visible(state, installation, attempt, report, ui)
    after = application(state, installation, attempt, "after")
    _, receiver = peer_state(owner)
    if (
        after["mainPid"] != report["application"]["mainPid"]
        or after["loadedSha256"] != report["application"]["loadedSha256"]
        or receiver["State"]["StartedAt"] != report["receiverStartedAt"]
    ):
        message = (
            "Installed service or independent receiver restarted "
            "during UI journey"
        )
        raise RuntimeError(message)
    report["applicationAfter"] = after


def _visible(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Ordered visible input and assertions must not be retried independently.
    state: dict[str, Any],
    installation: dict[str, Any],
    attempt: Path,
    report: dict[str, Any],
    ui: GuestUI,
) -> None:
    session = desktop_session(state, attempt)
    payload = Path(report["submitted"]["sourcePath"])
    received = (
        Path(state["directory"]) / "peer-cases" / "received" / payload.name
    )
    copy_payload(ui, installation, report)
    plugin_id = installation["candidate"]["plugin"]["id"]
    guest_command(
        state,
        session + shlex.join(["omarchy-shell", plugin_id, "open"]),
        attempt,
    )
    report["captureControl"] = ui.require("Choose a device")
    report["action"] = "capture copied file through visible panel paste"
    vm_control.save(attempt / "result.json", report)
    ui.keys("ctrl", "v")
    report["panelPaste"] = {
        "keys": ["ctrl", "v"],
        "route": "owned-guest-qmp",
        "observedAt": now(),
        "resultFrame": ui.frame["frame"],
    }
    baseline = _submit_visible_peer(ui, report)
    report["action"] = "correlate UI-created share with receiver bytes"
    scenario._completion(state, attempt, report, baseline, received)  # ruff: ignore[private-member-access] - Reuse receiver consent, metadata, exact bytes and public terminal verification.
    _dismiss_completed_share(ui, report)


def _submit_visible_peer(
    ui: GuestUI, report: dict[str, Any]
) -> tuple[str, str]:
    state, attempt = ui.state, ui.directory.parent
    snapshot = public_snapshot(state, attempt)
    if snapshot.get("discovery") == "timed_out":
        search = ui.require("Search again")
        report["searchAgain"] = search
        ui.click(search)
    report["selectedPeer"] = discover(state, attempt, report)
    prefix = report["selectedPeer"]["name"].partition("-")[0]
    unique_peer_prefix(
        public_snapshot(state, attempt), report["selectedPeer"], prefix
    )
    control = ui.require(prefix, prefix=True)
    snapshot = public_snapshot(state, attempt)
    unique_peer_prefix(snapshot, report["selectedPeer"], prefix)
    report["peerControl"] = {"prefix": prefix, "observation": control}
    if snapshot.get("active_share"):
        message = "Unexpected share before visible peer click"
        raise RuntimeError(message)
    baseline = receiver_logs(
        report["peer"]["containers"]["peer"], attempt, "before"
    )
    report["action"] = "submit public send to selected peer"
    report["submittedAt"] = now()
    vm_control.save(attempt / "result.json", report)
    ui.click(control)
    _require_created_share(ui, report)
    vm_control.save(attempt / "result.json", report)
    return baseline


def _require_created_share(ui: GuestUI, report: dict[str, Any]) -> None:
    state, attempt = ui.state, ui.directory.parent
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        share = public_snapshot(state, attempt).get("active_share")
        if share:
            report["shareId"] = share["id_string"]
            break
        time.sleep(0.5)
    else:
        message = "Visible peer input did not create a new public share"
        raise RuntimeError(message)


def _dismiss_completed_share(ui: GuestUI, report: dict[str, Any]) -> None:
    state, attempt = ui.state, ui.directory.parent
    sent = ui.require("Sent")
    done = ui.require("Done")
    report["panelTerminal"] = {
        "title": sent,
        "done": done,
        "shareId": report["shareId"],
        "observedAt": now(),
    }
    final = public_snapshot(state, attempt).get("active_share")
    if (
        not final
        or final["id_string"] != report["shareId"]
        or final["phase"] != "completed"
    ):
        message = "Panel terminal observation lost the completed public share"
        raise RuntimeError(message)
    report["action"] = "dismiss completed share through visible Done control"
    vm_control.save(attempt / "result.json", report)
    ui.click(done)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if public_snapshot(state, attempt).get("active_share") is None:
            report["panelDismissed"] = {
                "observedAt": now(),
                "resultFrame": ui.frame["frame"],
            }
            break
        time.sleep(0.5)
    else:
        message = "Visible Done input did not dismiss the completed share"
        raise RuntimeError(message)


def _installation_owner(installation: dict[str, Any]) -> Path:
    if installation.get("status") != "ready":
        message = "A successful clean-install installation.json is required"
        raise RuntimeError(message)
    os.environ["DESKTOP_LAB"] = installation["lab"]
    owner = lab_lifecycle.directory()
    if owner.resolve() != Path(installation["ownerDirectory"]).resolve():
        message = "Installation owner differs from selected lab"
        raise RuntimeError(message)
    return owner


def _start_attempt(
    path: Path, installation: dict[str, Any], owner: Path
) -> tuple[Path, dict[str, Any]]:
    attempt = owner / "scenarios" / str(uuid.uuid4())
    attempt.mkdir(parents=True, mode=0o700)
    return attempt, {
        "attemptId": attempt.name,
        "scenario": "file-to-reference-peer",
        "driver": "visible-panel",
        "lab": owner.name,
        "status": "failed",
        "startedAt": now(),
        "guestHome": installation["absence"]["home"],
        "installationManifest": str(path.resolve()),
        "installationManifestSha256": hashlib.sha256(
            path.read_bytes()
        ).hexdigest(),
        "sourceSha256": {
            name: hashlib.sha256(
                Path(__file__).with_name(name).read_bytes()
            ).hexdigest()
            for name in (
                "ui_scenario.py",
                "guest_ui.py",
                "guest_file_selection.py",
                "scenario.py",
                "scenario_state.py",
                "installed_ui_control.py",
                "installation_attestation.py",
            )
        },
    }


def _require_installed_guest(state: dict, installation: dict) -> None:
    if (
        state["uuid"] != installation["guest"]["uuid"]
        or state["pid"] != installation["guest"]["pid"]
    ):
        message = (
            "Installed guest process changed; use "
            "--reattest-installation for an owned resumed guest"
        )
        raise RuntimeError(message)


def _finish_report(attempt: Path, report: dict[str, Any]) -> None:
    report["finishedAt"] = now()
    vm_control.save(attempt / "result.json", report)
    print(f"Visible scenario {report['status']}; retained evidence: {attempt}")


def run(
    path: Path,
    *,
    cli_unavailable: bool = False,
    reattest_installation: bool = False,
) -> None:
    """Hold the lab lock and OCR worker claim through all assertions.

    Raises:
        KeyboardInterrupt: The operator interrupted the retained attempt.
    """
    installation = json.loads(path.read_text(encoding="utf-8"))
    owner = _installation_owner(installation)
    with (owner / "operation.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        attempt, report = _start_attempt(path, installation, owner)
        try:  # ruff: ignore[too-many-statements-in-try-clause] - Preserve the complete attempt on any ownership, admission or UI failure.
            state = deploy_native._running(owner)  # ruff: ignore[private-member-access] - Reuse the retained VM ownership check.
            if reattest_installation:
                report["installationAttestation"] = reattest(
                    state, installation, path, attempt
                )
            else:
                _require_installed_guest(state, installation)
            state = {
                **state,
                "cliExecutable": "/usr/bin/omarchy-quickshare",
                "daemonExecutable": "/usr/bin/omarchy-quickshare-daemon",
            }
            report.update({"guestUuid": state["uuid"], "qemuPid": state["pid"]})
            with resource_budget.scenario_claim(
                owner, cpus=1, memory_mib=512
            ) as claim:
                report["workerClaim"] = claim
                ui = InstalledUI(state, attempt)
                exercise(
                    state,
                    installation,
                    attempt,
                    report,
                    ui,
                    cli_unavailable=cli_unavailable,
                )
                report["status"] = "passed"
        except (Exception, KeyboardInterrupt) as error:
            report["error"] = f"{type(error).__name__}: {error}"
            raise
        finally:
            _finish_report(attempt, report)


def main() -> None:
    """Require an explicit successful installation manifest."""
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installation", required=True, type=Path)
    parser.add_argument(
        "--cli-unavailable",
        action="store_true",
        help="Remove the selected installed CLI's execute bits during visible "
        "panel work and restore its exact mode before the final native probe.",
    )
    parser.add_argument(
        "--reattest-installation",
        action="store_true",
        help="Re-probe the same owned installation after a guest restart; "
        "retain a separate attestation without changing the original manifest.",
    )
    options = parser.parse_args()
    run(
        options.installation,
        cli_unavailable=options.cli_unavailable,
        reattest_installation=options.reattest_installation,
    )


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:  # ruff: ignore[blind-except] - Standalone boundary reports the retained failure.
        print(f"Visible scenario failed: {error}", file=sys.stderr)
        sys.exit(1)
