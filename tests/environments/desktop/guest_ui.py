"""Observe owned guest frames and route bounded QMP input, never host input."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import csv
import io
import json
import os
import re
import shlex
import time
from pathlib import Path
from typing import Any

import deploy_native
import vm_control
from scenario_state import guest_command, now

OCR_SCALE = 2

SESSION_PROBE = """import json, os, subprocess
from pathlib import Path
instances = json.loads(subprocess.check_output(
    ['hyprctl', 'instances', '-j'], text=True, timeout=10))
if len(instances) != 1:
    raise RuntimeError('Cannot identify one guest compositor')
instance = instances[0]
runtime = Path(os.environ['XDG_RUNTIME_DIR'])
sockets = [path.name for path in runtime.glob('wayland-*') if path.is_socket()]
if len(sockets) != 1:
    raise RuntimeError('Cannot identify one guest Wayland socket')
if instance.get('wl_socket') and instance['wl_socket'] != sockets[0]:
    raise RuntimeError('Guest compositor and Wayland socket disagree')
print(json.dumps({
    'HYPRLAND_INSTANCE_SIGNATURE': instance['instance'],
    'WAYLAND_DISPLAY': sockets[0],
    'XDG_SESSION_TYPE': 'wayland',
    'XDG_CURRENT_DESKTOP': 'Hyprland',
}))
"""


def desktop_session(state: dict[str, Any], attempt: Path) -> str:
    """Select one guest compositor and Wayland socket for GUI commands.

    Returns:
        Quoted exports extending the native user-bus environment.
    """
    environment = json.loads(
        guest_command(
            state,
            deploy_native.SESSION + shlex.join(["python", "-c", SESSION_PROBE]),
            attempt,
        )
    )
    vm_control.save(attempt / "desktop-session.json", environment)
    return (
        deploy_native.SESSION
        + "set -Eeuo pipefail\n"
        + "".join(
            f"export {key}={shlex.quote(value)}\n"
            for key, value in environment.items()
        )
    )


def normalized(text: str) -> str:
    """Normalize OCR whitespace and punctuation, not letters or digits.

    Returns:
        Lowercase alphanumeric OCR text.
    """
    return re.sub(r"[^a-z0-9]", "", text.lower())


def phrase_matches(
    line: dict[str, Any], text: str, *, prefix: bool = False
) -> list[dict[str, Any]]:
    """Locate every contiguous word phrase within one OCR line.

    Returns:
        All matching phrases with bounds restricted to their matched words.
    """
    expected = [normalized(word) for word in text.split() if normalized(word)]
    if not expected:
        return []
    words = [word for word in line["words"] if normalized(word["text"])]
    matches = []
    for start in range(len(words) - len(expected) + 1):
        selected = words[start : start + len(expected)]
        actual = [normalized(word["text"]) for word in selected]
        if actual[:-1] != expected[:-1]:
            continue
        if (
            not actual[-1].startswith(expected[-1])
            if prefix
            else actual[-1] != expected[-1]
        ):
            continue
        matches.append({
            "text": " ".join(word["text"] for word in selected),
            "words": selected,
            "bounds": [
                min(word["bounds"][0] for word in selected),
                min(word["bounds"][1] for word in selected),
                max(word["bounds"][2] for word in selected),
                max(word["bounds"][3] for word in selected),
            ],
        })
    return matches


def visible_file(
    frame: dict[str, Any], window: dict[str, Any], prefix: str
) -> dict[str, Any] | None:
    """Find a filename prefix strictly inside the observed focused window.

    The caller verifies a single unscaled guest monitor before using bounds.

    Returns:
        One visible file label, or None while directory contents are loading.

    Raises:
        RuntimeError: More than one visible entry matches the unique prefix.
    """
    x, y = window["at"]
    width, height = window["size"]
    matches = []
    for control in frame["controls"]:
        left, top, right, bottom = control["bounds"]
        if (
            x <= left < right <= x + width
            and y <= top < bottom <= y + height
            and normalized(control["text"]).startswith(normalized(prefix))
        ):
            matches.append({**control, "frame": frame["frame"]})
    if len(matches) > 1:
        message = "Ambiguous visible synthetic file entry in focused window"
        raise RuntimeError(message)
    return matches[0] if matches else None


class GuestUI:
    """One serial OCR worker with retained frames and observed input targets."""

    def __init__(self, state: dict[str, Any], attempt: Path) -> None:
        """Retain the owned guest and a private observation directory."""
        self.state = state
        self.directory = attempt / "ui"
        self.directory.mkdir(mode=0o700)
        self.sequence = 0
        self.frame: dict[str, Any] = {}
        self.environment = {
            **os.environ,
            "OMP_THREAD_LIMIT": "1",
            "MAGICK_THREAD_LIMIT": "1",
        }

    def record(self, event: dict[str, Any]) -> None:
        """Persist routing and observation evidence before the next action."""
        with (self.directory / "events.jsonl").open(
            "a", encoding="utf-8"
        ) as stream:
            stream.write(
                json.dumps({
                    "at": now(),
                    "guestUuid": self.state["uuid"],
                    "qemuPid": self.state["pid"],
                    **event,
                })
                + "\n"
            )

    def capture(self) -> dict[str, Any]:
        """Capture a fresh guest framebuffer without OCR.

        Returns:
            Frame path, dimensions and empty controls invalidating old bounds.
        """
        self.sequence += 1
        base = self.directory / f"{self.sequence:04d}"
        ppm = base.with_suffix(".ppm")
        png = base.with_suffix(".png")
        vm_control.qmp(self.state, "screendump", {"filename": str(ppm)})
        vm_control.run(
            ["magick", "-limit", "thread", "1", ppm, png],
            env=self.environment,
            capture_output=True,
            timeout=15,
        )
        dimensions = vm_control.output(
            ["magick", "identify", "-format", "%w %h", png],
            env=self.environment,
            timeout=10,
        )
        width, height = map(int, dimensions.split())
        self.frame = {
            "frame": str(png),
            "width": width,
            "height": height,
            "controls": [],
            "observedAt": now(),
        }
        self.record({"observation": self.frame})
        return self.frame

    def observe(self) -> dict[str, Any]:
        """Capture and OCR a fresh framebuffer with bounded host resources.

        Returns:
            Frame path, dimensions and observed text bounds.
        """
        captured = self.capture()
        png = Path(captured["frame"])
        base = png.with_suffix("")
        # Native small labels lose glyph detail at screenshot resolution.
        # A fixed 2x raster passed the retained peer-label OCR experiment;
        # keep coordinates and evidence anchored to the original framebuffer.
        ocr_image = base.with_suffix(".ocr.png")
        vm_control.run(
            [
                "magick",
                "-limit",
                "thread",
                "1",
                png,
                "-resize",
                f"{OCR_SCALE * 100}%",
                ocr_image,
            ],
            env=self.environment,
            capture_output=True,
            timeout=15,
        )
        result = vm_control.run(
            ["tesseract", ocr_image, "stdout", "--psm", "11", "tsv"],
            env=self.environment,
            capture_output=True,
            text=True,
            timeout=20,
        )
        base.with_suffix(".tsv").write_text(result.stdout, encoding="utf-8")
        base.with_suffix(".log").write_text(result.stderr, encoding="utf-8")
        lines: dict[tuple[str, ...], list[dict[str, Any]]] = {}
        for row in csv.DictReader(io.StringIO(result.stdout), delimiter="\t"):
            if not row["text"].strip() or float(row["conf"]) < 0:
                continue
            key = tuple(
                row[field]
                for field in ("page_num", "block_num", "par_num", "line_num")
            )
            lines.setdefault(key, []).append(row)
        controls = []
        for words in lines.values():
            left = min(int(word["left"]) for word in words)
            top = min(int(word["top"]) for word in words)
            right = max(
                int(word["left"]) + int(word["width"]) for word in words
            )
            bottom = max(
                int(word["top"]) + int(word["height"]) for word in words
            )
            controls.append({
                "text": " ".join(word["text"] for word in words),
                "words": [
                    {
                        "text": word["text"],
                        "confidence": float(word["conf"]),
                        "bounds": [
                            int(word["left"]) // OCR_SCALE,
                            int(word["top"]) // OCR_SCALE,
                            (
                                int(word["left"])
                                + int(word["width"])
                                + OCR_SCALE
                                - 1
                            )
                            // OCR_SCALE,
                            (
                                int(word["top"])
                                + int(word["height"])
                                + OCR_SCALE
                                - 1
                            )
                            // OCR_SCALE,
                        ],
                    }
                    for word in words
                ],
                "bounds": [
                    left // OCR_SCALE,
                    top // OCR_SCALE,
                    (right + OCR_SCALE - 1) // OCR_SCALE,
                    (bottom + OCR_SCALE - 1) // OCR_SCALE,
                ],
            })
        self.frame = {
            **captured,
            "ocrImage": str(ocr_image),
            "ocrScale": OCR_SCALE,
            "controls": controls,
            "recognizedAt": now(),
        }
        self.record({"observation": self.frame})
        return self.frame

    def require(
        self, text: str, *, timeout: float = 15, prefix: bool = False
    ) -> dict[str, Any]:
        """Locate one exact word phrase or an explicit unique prefix.

        Returns:
            Unique observed control with its source frame.

        Raises:
            RuntimeError: The control is absent or ambiguous.
        """
        deadline = time.monotonic() + timeout
        while True:
            frame = self.observe()
            matches = [
                match
                for line in frame["controls"]
                for match in phrase_matches(line, text, prefix=prefix)
            ]
            if len(matches) > 1:
                message = f"Ambiguous visible control {text!r}: {matches}"
                raise RuntimeError(message)
            if matches:
                return {**matches[0], "frame": frame["frame"]}
            if time.monotonic() >= deadline:
                message = (
                    f"Missing visible control {text!r}; "
                    f"last frame {frame['frame']}"
                )
                raise RuntimeError(message)
            time.sleep(0.5)

    def click(self, control: dict[str, Any]) -> None:
        """Click bounds from the latest observed guest frame only.

        Raises:
            RuntimeError: Control bounds are stale or outside the guest.
        """
        if control["frame"] != self.frame["frame"]:
            message = "Refusing stale guest control bounds"
            raise RuntimeError(message)
        left, top, right, bottom = control["bounds"]
        x, y = (left + right) // 2, (top + bottom) // 2
        if not (0 <= x < self.frame["width"] and 0 <= y < self.frame["height"]):
            message = "Observed guest control is outside framebuffer"
            raise RuntimeError(message)
        self.record({
            "input": "pointer-click",
            "route": "owned-guest-qmp",
            "control": control,
            "coordinates": [x, y],
        })
        vm_control.qmp(
            self.state,
            "input-send-event",
            {
                "events": [
                    {
                        "type": "abs",
                        "data": {
                            "axis": "x",
                            "value": round(
                                x * 32767 / (self.frame["width"] - 1)
                            ),
                        },
                    },
                    {
                        "type": "abs",
                        "data": {
                            "axis": "y",
                            "value": round(
                                y * 32767 / (self.frame["height"] - 1)
                            ),
                        },
                    },
                    {"type": "btn", "data": {"button": "left", "down": True}},
                ]
            },
        )
        # USB input is synchronized per QMP request; do not coalesce press
        # and release into a final unpressed tablet state.
        try:
            time.sleep(0.1)
        finally:
            vm_control.qmp(
                self.state,
                "input-send-event",
                {
                    "events": [
                        {
                            "type": "btn",
                            "data": {"button": "left", "down": False},
                        },
                    ]
                },
            )
        time.sleep(0.3)
        self.capture()

    def keys(self, *keys: str) -> None:
        """Send a chord to the observed guest and capture its result.

        Raises:
            RuntimeError: No guest frame has been observed.
        """
        if not self.frame:
            message = "Guest keyboard input needs a fresh observation"
            raise RuntimeError(message)
        self.record({
            "input": "keyboard",
            "route": "owned-guest-qmp",
            "keys": keys,
            "frame": self.frame["frame"],
        })
        vm_control.qmp(
            self.state,
            "send-key",
            {
                "keys": [{"type": "qcode", "data": key} for key in keys],
                "hold-time": 100,
            },
        )
        time.sleep(0.3)
        self.capture()
