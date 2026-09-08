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
from guest_ui import GuestUI, desktop_session, normalized, visible_file
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
    deadline = time.monotonic() + 30
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


def exercise(  # ruff: ignore[too-many-arguments, too-many-positional-arguments, too-many-statements, too-many-branches, too-many-locals, complex-structure] - Keep ordered input and its identity assertions together; no transfer step may be retried independently.
    state: dict[str, Any],
    installation: dict[str, Any],
    attempt: Path,
    report: dict[str, Any],
    ui: GuestUI,
) -> None:
    """Drive file-manager copy and panel peer selection through QMP.

    Raises:
        RuntimeError: Visible input, share identity or completion disagrees.
    """
    owner = Path(state["directory"])
    session = desktop_session(state, attempt)
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
    report["action"] = "select synthetic file in visible file manager"
    # The shipped panel has no file chooser. Its normal file path consumes
    # text/uri-list from a file manager copy, then a peer click or Ctrl+V.
    # xdg-open only prepares visibility; QMP selects and copies the file.
    guest_command(
        state,
        session
        + "xdg-open "
        + shlex.quote(
            str(Path(installation["absence"]["home"]) / Path(guest_path).parent)
        )
        + " >/dev/null 2>&1 &",
        attempt,
    )
    monitors = json.loads(
        guest_command(state, session + "hyprctl monitors -j", attempt)
    )
    if (
        len(monitors) != 1
        or monitors[0]["scale"] != 1
        or monitors[0]["x"] != 0
        or monitors[0]["y"] != 0
        or monitors[0]["transform"] != 0
    ):
        message = "File readiness requires one unscaled guest monitor"
        raise RuntimeError(message)
    # Cold Nautilus startup took 23 seconds on the single-CPU reference guest.
    deadline = time.monotonic() + 30
    maximized = False
    while True:
        frame = ui.observe()
        if (
            frame["width"] != monitors[0]["width"]
            or frame["height"] != monitors[0]["height"]
        ):
            message = "Guest monitor and observed framebuffer bounds disagree"
            raise RuntimeError(message)
        focused = json.loads(
            guest_command(
                state,
                session + "hyprctl activewindow -j",
                attempt,
            )
        )
        if (
            attempt.name in focused.get("title", "")
            and focused.get("mapped")
            and not focused.get("hidden")
            and any(
                name in focused.get("class", "").lower()
                for name in ("thunar", "nautilus", "dolphin", "pcmanfm")
            )
        ):
            if not maximized and not focused.get("fullscreen"):
                report["fileManagerBeforeMaximize"] = focused
                ui.record({
                    "setup": "maximize-private-file-manager",
                    "route": "owned-guest-hyprland",
                    "window": focused["address"],
                    "frame": frame["frame"],
                })
                guest_command(
                    state,
                    session
                    + shlex.join([
                        "hyprctl",
                        "dispatch",
                        'hl.dsp.window.fullscreen({ mode = "maximized" })',
                    ]),
                    attempt,
                )
                maximized = True
                # Discard pre-dispatch geometry and pixels. Observe the
                # actual maximized window before locating any file control.
                continue
            # Locate the human-readable stem only within the exact UUID
            # window; ambiguity fails and the copied URI must match in full.
            entry = visible_file(frame, focused, payload.name.partition("-")[0])
            if entry:
                break
        if time.monotonic() >= deadline:
            message = (
                "Synthetic file entry is not visible in the "
                "focused private file-manager window"
            )
            raise RuntimeError(message)
        time.sleep(0.5)
    report["fileManagerFocus"] = focused
    report["fileManagerEntry"] = entry
    ui.click(entry)
    ui.keys("ctrl", "c")
    copied = guest_command(
        state,
        session + "timeout 5 wl-paste --type text/uri-list --no-newline",
        attempt,
    )
    expected_uri = (Path(installation["absence"]["home"]) / guest_path).as_uri()
    if copied.strip().splitlines() != [expected_uri]:
        message = (
            "Visible file-manager copy did not select exactly "
            "the synthetic file"
        )
        raise RuntimeError(message)
    report["copiedUri"] = copied.strip()
    report["fileSelection"] = {
        "name": payload.name,
        "route": "guest-file-manager-copy",
        "at": now(),
    }
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
    vm_control.save(attempt / "result.json", report)
    report["action"] = "correlate UI-created share with receiver bytes"
    scenario._completion(state, attempt, report, baseline, received)  # ruff: ignore[private-member-access] - Reuse receiver consent, metadata, exact bytes and public terminal verification.
    sent = ui.require("Sent")
    done = ui.require("Done")
    report["panelTerminal"] = {
        "title": sent,
        "done": done,
        "shareId": report["shareId"],
        "observedAt": now(),
    }
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


def run(path: Path) -> None:
    """Hold the lab lock and OCR worker claim through all assertions.

    Raises:
        RuntimeError: Installation or live guest ownership does not match.
        KeyboardInterrupt: The operator interrupted the retained attempt.
    """
    installation = json.loads(path.read_text(encoding="utf-8"))
    if installation.get("status") != "ready":
        message = "A successful clean-install installation.json is required"
        raise RuntimeError(message)
    os.environ["DESKTOP_LAB"] = installation["lab"]
    owner = lab_lifecycle.directory()
    if owner.resolve() != Path(installation["ownerDirectory"]).resolve():
        message = "Installation owner differs from selected lab"
        raise RuntimeError(message)
    with (owner / "operation.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        attempt = owner / "scenarios" / str(uuid.uuid4())
        attempt.mkdir(parents=True, mode=0o700)
        report: dict[str, Any] = {
            "attemptId": attempt.name,
            "scenario": "file-to-reference-peer",
            "driver": "visible-panel",
            "lab": owner.name,
            "status": "failed",
            "startedAt": now(),
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
                    "scenario.py",
                    "scenario_state.py",
                )
            },
        }
        state = None
        ui = None
        try:  # ruff: ignore[too-many-statements-in-try-clause] - Preserve the complete attempt on any ownership, admission or UI failure.
            state = deploy_native._running(owner)  # ruff: ignore[private-member-access] - Reuse the retained VM ownership check.
            if (
                state["uuid"] != installation["guest"]["uuid"]
                or state["pid"] != installation["guest"]["pid"]
            ):
                message = "Installed guest process identity changed"
                raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Outer handler retains the failed attempt then re-raises.
            state = {
                **state,
                "cliExecutable": "/usr/bin/omarchy-quickshare",
            }
            report.update({"guestUuid": state["uuid"], "qemuPid": state["pid"]})
            with resource_budget.scenario_claim(
                owner, cpus=1, memory_mib=512
            ) as claim:
                report["workerClaim"] = claim
                ui = GuestUI(state, attempt)
                exercise(state, installation, attempt, report, ui)
                report["status"] = "passed"
        except (Exception, KeyboardInterrupt) as error:
            report["error"] = f"{type(error).__name__}: {error}"
            raise
        finally:
            report["finishedAt"] = now()
            vm_control.save(attempt / "result.json", report)
            print(
                f"Visible scenario {report['status']}; "
                f"retained evidence: {attempt}"
            )


def main() -> None:
    """Require an explicit successful installation manifest."""
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--installation", required=True, type=Path)
    options = parser.parse_args()
    run(options.installation)


if __name__ == "__main__":
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:  # ruff: ignore[blind-except] - Standalone boundary reports the retained failure.
        print(f"Visible scenario failed: {error}", file=sys.stderr)
        sys.exit(1)
