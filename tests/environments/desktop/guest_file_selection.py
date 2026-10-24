"""Select an owned payload through the actual guest file manager."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import json
import shlex
import time
from pathlib import Path
from typing import Any

from guest_ui import GuestUI, desktop_session, visible_file
from scenario_state import guest_command, now


def _window(ui: GuestUI, session: str) -> dict[str, Any]:
    return json.loads(
        guest_command(
            ui.state, session + "hyprctl activewindow -j", ui.directory.parent
        )
    )


def _private(window: dict[str, Any], attempt: Path) -> bool:
    return bool(
        window.get("title") == attempt.name
        and window.get("mapped")
        and window.get("visible")
        and window.get("acceptsInput")
        and not window.get("hidden")
        and any(
            name in window.get("class", "").lower()
            for name in ("thunar", "nautilus", "dolphin", "pcmanfm")
        )
    )


def _focused(ui: GuestUI, session: str, expected: dict[str, Any]) -> None:
    current = _window(ui, session)
    ui.record({
        "focusCheck": current,
        "expectedWindow": expected["address"],
        "frame": ui.frame["frame"],
    })
    if not _private(current, ui.directory.parent) or any(
        current.get(key) != expected.get(key)
        for key in ("address", "pid", "stableId", "at", "size", "workspace")
    ):
        message = "Private file-manager focus or geometry changed before input"
        raise RuntimeError(message)


def _monitor(ui: GuestUI, session: str) -> dict[str, Any]:
    attempt = ui.directory.parent
    monitors = json.loads(
        guest_command(ui.state, session + "hyprctl monitors -j", attempt)
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
    return monitors[0]


def _maximize(ui: GuestUI, session: str, focused: dict[str, Any]) -> None:
    ui.record({
        "setup": "maximize-private-file-manager",
        "route": "owned-guest-hyprland",
        "window": focused["address"],
        "frame": ui.frame["frame"],
    })
    guest_command(
        ui.state,
        session
        + shlex.join([
            "hyprctl",
            "dispatch",
            'hl.dsp.window.fullscreen({ mode = "maximized" })',
        ]),
        ui.directory.parent,
    )


def _ready(
    ui: GuestUI, session: str, report: dict[str, Any]
) -> tuple[dict[str, Any], dict[str, Any]]:
    attempt = ui.directory.parent
    payload = Path(report["submitted"]["sourcePath"])
    monitor = _monitor(ui, session)
    deadline = time.monotonic() + 30
    maximized = False
    while True:
        frame = ui.observe()
        if (
            frame["width"] != monitor["width"]
            or frame["height"] != monitor["height"]
        ):
            message = "Guest monitor and observed framebuffer bounds disagree"
            raise RuntimeError(message)
        focused = _window(ui, session)
        if _private(focused, attempt):
            if not maximized and not focused.get("fullscreen"):
                report["fileManagerBeforeMaximize"] = focused
                _maximize(ui, session, focused)
                maximized = True
                continue
            entry = visible_file(frame, focused, payload.name.partition("-")[0])
            if entry:
                return focused, entry
        if time.monotonic() >= deadline:
            message = (
                "Synthetic file is not visible in its focused private window"
            )
            raise RuntimeError(message)
        time.sleep(0.5)


def _close_popup(
    ui: GuestUI,
    session: str,
    report: dict[str, Any],
) -> None:
    identity = report["application"]["plugin"]["loadedIdentity"]
    plugin_id = identity["id"]
    report["popupPreparation"] = {
        "identity": identity,
        "before": ui.capture(),
        "command": "close",
    }
    guest_command(
        ui.state,
        session + shlex.join(["omarchy-shell", plugin_id, "close"]),
        ui.directory.parent,
    )
    report["popupPreparation"]["after"] = ui.capture()


def _select_payload(
    ui: GuestUI, session: str, report: dict[str, Any]
) -> dict[str, Any]:
    focused, entry = _ready(ui, session, report)
    report["fileManagerFocus"] = focused
    report["fileManagerEntry"] = entry
    _focused(ui, session, focused)
    ui.click(entry)
    # Retained clicks sometimes left no selection. Select this private
    # single-payload directory through the file manager, then require visible
    # selection before copy. The exact URI check rejects any extra selection.
    _focused(ui, session, focused)
    ui.keys("ctrl", "a")
    selected = ui.require("selected")
    left, top, right, bottom = selected["bounds"]
    x, y = focused["at"]
    width, height = focused["size"]
    if not (x <= left < right <= x + width and y <= top < bottom <= y + height):
        message = "Selection feedback is outside the private file manager"
        raise RuntimeError(message)
    report["fileManagerSelection"] = selected
    return focused


def copy_payload(
    ui: GuestUI, installation: dict[str, Any], report: dict[str, Any]
) -> None:
    """Close only this plugin's popup and visibly select/copy the owned file.

    Raises:
        RuntimeError: Popup identity, focus, visible selection or URI differs.
    """
    state, attempt = ui.state, ui.directory.parent
    session = desktop_session(state, attempt)
    payload = Path(report["submitted"]["sourcePath"])
    guest_path = Path(report["submitted"]["guestPath"])
    expected_uri = (Path(installation["absence"]["home"]) / guest_path).as_uri()
    plugin_id = installation["candidate"]["plugin"]["id"]
    if report["application"]["plugin"]["loadedIdentity"]["id"] != plugin_id:
        message = "Cannot identify this installation's popup for preparation"
        raise RuntimeError(message)
    _close_popup(ui, session, report)
    report["action"] = "select synthetic file in visible file manager"
    guest_command(
        state,
        session
        + "xdg-open "
        + shlex.quote(
            str(Path(installation["absence"]["home"]) / guest_path.parent)
        )
        + " >/dev/null 2>&1 &",
        attempt,
    )
    focused = _select_payload(ui, session, report)
    _focused(ui, session, focused)
    ui.keys("ctrl", "c")
    _focused(ui, session, focused)
    copied = guest_command(
        state,
        session + "timeout 5 wl-paste --type text/uri-list --no-newline",
        attempt,
    )
    if copied.strip().splitlines() != [expected_uri]:
        message = "Copied URI differs from the single synthetic payload"
        raise RuntimeError(message)
    report["copiedUri"] = copied.strip()
    report["fileSelection"] = {
        "name": payload.name,
        "route": "guest-file-manager-copy",
        "at": now(),
    }
