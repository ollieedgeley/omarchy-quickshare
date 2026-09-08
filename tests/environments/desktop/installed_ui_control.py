"""CLI execute-bit proof for an owned installed guest."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import json
import shlex
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from typing import Any

import deploy_native
import vm_control
from guest_ui import GuestUI
from scenario_state import guest_command, now

CLI_MODE = """import hashlib, json, os, stat, sys
path, action, encoded = sys.argv[1:]
expected = json.loads(encoded)
fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
try:
    info = os.fstat(fd)
    if not stat.S_ISREG(info.st_mode):
        raise RuntimeError('Installed CLI is not a regular file')
    with os.fdopen(os.dup(fd), 'rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    identity = dict(path=path, device=info.st_dev, inode=info.st_ino,
                    uid=info.st_uid, gid=info.st_gid, size=info.st_size,
                    sha256=digest)
    if expected and identity != expected['identity']:
        raise RuntimeError('Selected installed CLI file identity changed')
    mode = stat.S_IMODE(info.st_mode)
    if action == 'disable':
        if mode != expected['mode'] or not mode & 0o111:
            raise RuntimeError('Installed CLI mode changed before disable')
        os.fchmod(fd, mode & ~0o111)
    elif action == 'restore':
        os.fchmod(fd, expected['mode'])
    elif action != 'inspect':
        raise RuntimeError('Unknown CLI proof action')
    actual = stat.S_IMODE(os.fstat(fd).st_mode)
    linked = os.stat(path, follow_symlinks=False)
    if (linked.st_dev, linked.st_ino) != (info.st_dev, info.st_ino):
        raise RuntimeError('Installed CLI path changed during mode operation')
    print(json.dumps(dict(identity=identity, mode=actual,
                         executable=os.access(path, os.X_OK))))
finally:
    os.close(fd)
"""

PRESERVED = """import hashlib, json, sys
from pathlib import Path
home, pid = sys.argv[1:]
config = Path(home) / '.config/omarchy-quickshare/config.toml'
started = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[19]
digest = (
    hashlib.sha256(config.read_bytes()).hexdigest() if config.exists() else None
)
print(json.dumps({
    'daemonStart': started,
    'daemonExecutable': str(Path(f'/proc/{pid}/exe').readlink()),
    'configSha256': digest,
}))
"""


class InstalledUI(GuestUI):
    """Keep CLI-unavailable checks at real framebuffer and input boundaries."""

    proof: Any = None

    def capture(self) -> dict[str, Any]:
        """Check the CLI before capturing.

        Returns:
            Actual guest framebuffer evidence.
        """
        if self.proof is not None:
            self.proof.check("before-frame")
        return super().capture()

    def record(self, event: dict[str, Any]) -> None:
        """Check before QMP input and after retained framebuffer evidence."""
        if self.proof is not None and (
            "input" in event or "observation" in event
        ):
            self.proof.check(event.get("input", "after-frame"))
        super().record(event)


class _CLIProof:
    def __init__(self, state: dict, attempt: Path, report: dict) -> None:
        self.state = state
        self.attempt = attempt
        self.report = report
        self.original: dict = {}
        self.evidence: dict = {"checks": []}
        report["cliUnavailable"] = self.evidence

    def command(self, action: str) -> dict:
        if not vm_control.running(self.state):
            message = "Owned guest identity was lost during CLI proof"
            raise RuntimeError(message)
        args = ["timeout", "10"]
        prefix = ""
        if action in {"disable", "restore"}:
            # Use the same disposable guest credential as clean_install.
            prefix = "printf '%s\\n' omarchy | "
            args += ["sudo", "-S", "--"]
        args += [
            "python",
            "-c",
            CLI_MODE,
            self.state["cliExecutable"],
            action,
            json.dumps(self.original),
        ]
        return json.loads(
            guest_command(
                self.state,
                deploy_native.SESSION
                + "set -Eeuo pipefail\n"
                + prefix
                + shlex.join(args),
                self.attempt,
            )
        )

    def preserved(self) -> dict:
        return json.loads(
            guest_command(
                self.state,
                deploy_native.SESSION
                + shlex.join([
                    "timeout",
                    "10",
                    "python",
                    "-c",
                    PRESERVED,
                    self.report["guestHome"],
                    str(self.report["application"]["mainPid"]),
                ]),
                self.attempt,
            )
        )

    def check(self, stage: str) -> None:
        actual = self.command("inspect")
        if (
            actual["mode"] != self.original["mode"] & ~0o111
            or actual["executable"]
            or self.preserved() != self.evidence["preservedBefore"]
        ):
            message = (
                "CLI became executable or daemon/config changed during UI proof"
            )
            raise RuntimeError(message)
        self.evidence["checks"].append({"at": now(), "stage": stage, **actual})
        vm_control.save(self.attempt / "cli-unavailable.json", self.evidence)


@contextmanager
def installed_cli_unavailable(  # ruff: ignore[too-many-arguments, too-many-positional-arguments, too-many-branches] - One restoration boundary spans the owned guest and visible journey.
    state: dict, attempt: Path, report: dict, ui: GuestUI, *, enabled: bool
) -> Iterator[dict[str, Any]]:
    """Restore the exact original mode even when visible work fails.

    Yields:
        Recorded file identity, modes and checks, or an empty disabled proof.

    Raises:
        TypeError: The UI lacks proof checks at input and frame boundaries.
        RuntimeError: Installed identity, mode or preserved state disagrees.
    """
    if not enabled:
        yield {}
        return
    if not isinstance(ui, InstalledUI):
        message = "CLI-unavailable proof requires InstalledUI input guards"
        raise TypeError(message)
    proof = _CLIProof(state, attempt, report)
    proof.original = proof.command("inspect")
    expected = report["application"]["components"]["cli"]
    if (
        proof.original["identity"]["path"] != expected["executable"]
        or proof.original["identity"]["sha256"] != expected["installedSha256"]
        or not proof.original["executable"]
        or not proof.original["mode"] & 0o111
    ):
        message = "CLI proof target does not match the installed native probe"
        raise RuntimeError(message)
    proof.evidence.update({
        "original": proof.original,
        "preservedBefore": proof.preserved(),
        "startedAt": now(),
    })
    vm_control.save(attempt / "cli-unavailable.json", proof.evidence)
    try:
        proof.evidence["disabled"] = proof.command("disable")
        ui.proof = proof
        proof.check("before-visible-work")
        yield proof.evidence
        proof.check("after-visible-work")
    finally:
        ui.proof = None
        try:
            restored = proof.command("restore")
            proof.evidence["restored"] = {"at": now(), **restored}
            if restored != proof.original:
                message = "Installed CLI did not regain its exact original mode"
                raise RuntimeError(message)
            proof.evidence["preservedAfter"] = proof.preserved()
            if (
                proof.evidence["preservedAfter"]
                != proof.evidence["preservedBefore"]
            ):
                message = "Daemon or configuration changed during CLI proof"
                raise RuntimeError(message)
        finally:
            vm_control.save(attempt / "cli-unavailable.json", proof.evidence)
