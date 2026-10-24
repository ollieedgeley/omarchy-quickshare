"""Install a candidate in a new guest through its distribution commands."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import argparse
import fcntl
import json
import os
import re
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Bounded transport failure handling.
import time
import uuid
from pathlib import Path
from typing import Any

import build_native
import candidate
import deploy_native
import deploy_panel
import environment
import lab_lifecycle
import vm_control

ATTEMPTS = vm_control.CACHE / "clean-installs"
PROBE = Path(__file__).with_name("clean_install_guest.py")


def guest(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Guest, command and retained stage evidence are independent inputs.
    state: dict[str, Any], script: str, attempt: Path, stage: str
) -> str:
    """Execute one pinned SSH stage with retained command/output evidence.

    Returns:
        The stage's standard output.

    Raises:
        subprocess.TimeoutExpired: Transport exceeded its existing deadline.
    """
    command = "bash -lc " + shlex.quote(
        deploy_native.SESSION + "set -Eeuo pipefail\n" + script
    )
    with (attempt / "commands.jsonl").open("a", encoding="utf-8") as stream:
        stream.write(
            json.dumps({
                "at": candidate.now(),
                "stage": stage,
                "transport": "pinned-ssh",
                "command": command,
            })
            + "\n"
        )
    try:
        return deploy_native._guest(  # ruff: ignore[private-member-access] - Reuse pinned transport and retained logs.
            state, command, attempt / (stage + ".log")
        )
    except subprocess.TimeoutExpired as error:
        with (attempt / (stage + ".log")).open("a", encoding="utf-8") as stream:
            for partial in (error.stdout, error.stderr):
                if partial:
                    stream.write(
                        partial.decode(errors="replace")
                        if isinstance(partial, bytes)
                        else partial
                    )
            stream.write("\nPinned SSH command timed out\n")
        raise


def probe(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Mode and nonce complement guest/stage evidence.
    state: dict[str, Any],
    manifest: dict[str, Any],
    attempt: Path,
    stage: str,
    mode: str,
    *extra: str,
) -> dict[str, Any]:
    """Execute a read-only installed-candidate identity probe.

    Returns:
        Decoded guest probe evidence.
    """
    return json.loads(
        guest(
            state,
            shlex.join([
                "python",
                manifest["guestStage"] + "/clean_install_guest.py",
                mode,
                manifest["guestStage"] + "/candidate.json",
                *extra,
            ]),
            attempt,
            stage,
        )
    )


def wait_probe(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Readiness mode is independent of guest and evidence ownership.
    state: dict[str, Any], manifest: dict[str, Any], attempt: Path, mode: str
) -> dict[str, Any]:
    """Wait for a complete read-only absence or installed-identity probe.

    Returns:
        The successful probe's evidence.

    Raises:
        RuntimeError: Candidate readiness did not arrive before the deadline.
    """
    deadline = time.monotonic() + 60
    last = "No readiness response"
    while time.monotonic() < deadline:
        try:
            extra = [str(uuid.uuid4())] if mode == "plugin" else []
            return probe(
                state, manifest, attempt, mode + "-readiness", mode, *extra
            )
        except (
            ValueError,
            KeyError,
            RuntimeError,
            OSError,
            subprocess.SubprocessError,
        ) as error:
            last = str(error)
        time.sleep(1)
    message = f"Fresh candidate {mode} readiness failed: {last}"
    raise RuntimeError(message)


def copy_inputs(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Candidate and attempt paths have distinct ownership.
    state: dict[str, Any],
    manifest: dict[str, Any],
    selected: Path,
    attempt: Path,
) -> None:
    """Stage candidate bytes and a read-only probe over pinned SSH.

    Raises:
        RuntimeError: A transferred input differs from its recorded digest.
    """
    stage = guest(
        state,
        "printf '%s\\n' \"$HOME/.cache/omarchy-quickshare-clean-install/"
        + attempt.name
        + '"',
        attempt,
        "stage-path",
    ).strip()
    manifest["guestStage"] = stage
    guest(
        state,
        "mkdir -p -- "
        + shlex.quote(str(Path(stage).parent))
        + "\nmkdir -- "
        + shlex.quote(stage),
        attempt,
        "stage-create",
    )
    args = [
        "scp",
        "-F",
        Path(state["directory"]) / "ssh.config",
        selected / "candidate.json",
        manifest["candidate"]["package"]["path"],
        manifest["candidate"]["plugin"]["bundle"],
        PROBE,
        f"desktop:{stage}/",
    ]
    candidate.command(args, vm_control.ROOT, attempt / "copy.log")
    for name, expected in [
        (
            Path(manifest["candidate"]["package"]["path"]).name,
            manifest["candidate"]["package"]["sha256"],
        ),
        ("plugin.bundle", manifest["candidate"]["plugin"]["bundleSha256"]),
        ("candidate.json", manifest["candidateManifestSha256"]),
        (PROBE.name, build_native.digest(PROBE)),
    ]:
        actual = guest(
            state,
            shlex.join(["sha256sum", str(Path(stage) / name)]),
            attempt,
            "copy-checksums",
        ).split()[0]
        if actual != expected:
            message = f"Guest input checksum mismatch: {name}"
            raise RuntimeError(message)


def install(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - Candidate and attempt paths have distinct ownership.
    state: dict[str, Any],
    manifest: dict[str, Any],
    selected: Path,
    attempt: Path,
) -> None:
    """Install shipped artifacts without a dev link or desktop replacement.

    Raises:
        RuntimeError: Installed identity or daemon continuity differs.
    """
    copy_inputs(state, manifest, selected, attempt)
    # A startup ping can precede a responsive plugin registry. Wait for the
    # actual absence probe, without weakening its checks or changing the guest.
    manifest["absence"] = wait_probe(state, manifest, attempt, "absent")
    # Plugin installation changes the shell's watched tree. Reuse the existing
    # lock-state precondition without restarting or replacing the desktop.
    manifest["lockPreflight"] = json.loads(
        guest(
            state,
            deploy_panel._python(deploy_panel.UNLOCKED),  # ruff: ignore[private-member-access] - Reuse the established guest lock-state guard.
            attempt,
            "lock-preflight",
        )
    )
    guest(
        state,
        "command -v omarchy-plugin-add; command -v omarchy-git-url-check; "
        "cat /usr/bin/omarchy-plugin-add; cat /usr/bin/omarchy-git-url-check; "
        "cat /etc/os-release; pacman -Q omarchy",
        attempt,
        "installer-source",
    )
    stage = Path(manifest["guestStage"])
    package = stage / Path(manifest["candidate"]["package"]["path"]).name
    guest(
        state,
        "printf '%s\\n' omarchy | sudo -S -- pacman -U --noconfirm -- "
        + shlex.quote(str(package)),
        attempt,
        "native-install",
    )
    guest(
        state,
        "systemctl --user daemon-reload\n"
        "systemctl --user enable --now omarchy-quickshare.service",
        attempt,
        "native-start",
    )
    manifest["native"] = wait_probe(state, manifest, attempt, "native")
    repository = stage / "plugin-repository"
    guest(
        state,
        shlex.join([
            "git",
            "clone",
            "--",
            str(stage / "plugin.bundle"),
            str(repository),
        ]),
        attempt,
        "plugin-source",
    )
    commit = guest(
        state,
        shlex.join(["git", "-C", str(repository), "rev-parse", "HEAD"]),
        attempt,
        "plugin-source-identity",
    ).strip()
    if commit != manifest["candidate"]["plugin"]["commit"]:
        message = "Guest plugin repository commit differs from candidate"
        raise RuntimeError(message)
    manifest["pluginInstallUrl"] = repository.as_uri()
    guest(
        state,
        shlex.join([
            "omarchy",
            "plugin",
            "add",
            repository.as_uri(),
            "--enable",
            "--yes",
        ]),
        attempt,
        "plugin-install",
    )
    manifest["plugin"] = wait_probe(state, manifest, attempt, "plugin")
    after = wait_probe(state, manifest, attempt, "native")
    if after["mainPid"] != manifest["native"]["mainPid"]:
        message = "Native daemon changed during plugin installation"
        raise RuntimeError(message)
    manifest["nativeAfterPlugin"] = after


def preparation(
    fingerprint: str,
) -> tuple[tuple[str, dict[str, Any], Path, Path], dict[str, Any]]:
    """Select an identified clean base without silently preparing another.

    Returns:
        Existing lifecycle inputs and the retained clean-base manifest.

    Raises:
        RuntimeError: Base identity, required files or firmware do not match.
    """
    if re.fullmatch(r"[0-9a-f]{64}", fingerprint) is None:
        message = "--base must be an identified prepared-base fingerprint"
        raise RuntimeError(message)
    base = vm_control.CACHE / "bases" / fingerprint
    prepared = json.loads((base / "prepared.json").read_text(encoding="utf-8"))
    if prepared["fingerprint"] != fingerprint:
        message = "Prepared base identity mismatch"
        raise RuntimeError(message)
    if prepared["inputs"]["manifest"] != vm_control.MANIFEST:
        message = "Selected base has a different guest/ISO contract"
        raise RuntimeError(message)
    for name in ["base.qcow2", "vars.fd", "known_hosts", "id_ed25519"]:
        path = base / name
        if path.is_symlink() or not path.is_file():
            message = f"Prepared base missing a regular owned input: {path}"
            raise RuntimeError(message)
    code, variables = environment.firmware()
    if build_native.digest(code) != prepared["inputs"]["firmware"]["code"]:
        message = "Selected base firmware differs from current firmware"
        raise RuntimeError(message)
    return (fingerprint, prepared["inputs"], code, variables), prepared


def _retain_guest(manifest: dict[str, Any], attempt: Path) -> None:
    # Launch can save ownership before failing. Recover only the owner this
    # attempt created, never an existing lab rejected by the freshness guard.
    state = manifest.get("guest")
    if manifest.get("createdOwner") and state is None:
        try:
            state = lab_lifecycle.active(Path(manifest["ownerDirectory"]))
            if state:
                manifest["guest"] = state
        except (OSError, ValueError, RuntimeError) as error:
            manifest["stateCaptureError"] = str(error)
    manifest["finishedAt"] = candidate.now()
    vm_control.save(attempt / "installation.json", manifest)
    if state and vm_control.running(state):
        deploy_native._journal(state, attempt)  # ruff: ignore[private-member-access] - Reuse retained service diagnostics.
        deploy_panel._journal(state, attempt)  # ruff: ignore[private-member-access] - Reuse retained shell diagnostics.
    print(f"Fresh-install guest and evidence retained: {attempt}")


def _started(owner: Path) -> dict[str, Any]:
    state = lab_lifecycle.active(owner)
    if state is None:
        message = "Lifecycle startup did not retain an owned guest"
        raise RuntimeError(message)
    return state


def run(  # ruff: ignore[too-many-arguments] - Optional job-selected evidence path preserves standalone behavior.
    selected: Path, lab: str, fingerprint: str, *, attempt: Path | None = None
) -> None:
    """Retain a fresh guest and installation evidence on success or failure.

    Raises:
        KeyboardInterrupt: The caller interrupted installation.
    """
    attempt = attempt or ATTEMPTS / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    manifest = {
        "schemaVersion": 1,
        "attemptId": attempt.name,
        "status": "failed",
        "startedAt": candidate.now(),
        "lab": lab,
        "baseFingerprint": fingerprint,
        "candidateDirectory": str(selected),
        "attemptDirectory": str(attempt),
    }
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Persist all partial installation outcomes.
        selected = selected.resolve(strict=True)
        manifest["candidate"] = candidate.load(selected)
        manifest["candidateManifestSha256"] = build_native.digest(
            selected / "candidate.json"
        )
        os.environ["DESKTOP_LAB"] = lab
        owner = lab_lifecycle.directory()
        manifest["ownerDirectory"] = str(owner)
        prepared, manifest["preparedBase"] = preparation(fingerprint)
        manifest["baseDirectory"] = str(
            vm_control.CACHE / "bases" / fingerprint
        )
        # mkdir without exist_ok is the freshness boundary. Even an empty old
        # owner is evidence, never an invitation to adopt or delete it.
        owner.parent.mkdir(parents=True, exist_ok=True)
        owner.mkdir()
        manifest["createdOwner"] = True
        identity = owner.stat()
        manifest["ownerIdentity"] = {
            "device": identity.st_dev,
            "inode": identity.st_ino,
        }
        vm_control.save(attempt / "installation.json", manifest)
        with (owner / "operation.lock").open("x", encoding="utf-8") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            vm_control.save(attempt / "installation.json", manifest)
            lab_lifecycle.start(owner, prepared)
            state = _started(owner)
            manifest["guest"] = state
            vm_control.save(attempt / "installation.json", manifest)
            install(state, manifest, selected, attempt)
            manifest["status"] = "ready"
    except (Exception, KeyboardInterrupt) as error:
        manifest["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        _retain_guest(manifest, attempt)
    print(json.dumps(manifest, indent=2))


def main() -> None:
    """Parse an explicit candidate, fresh owner and identified base."""
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument(
        "--lab",
        required=True,
        help="Unique new name; retained owners are refused",
    )
    parser.add_argument(
        "--base", required=True, help="Prepared clean-base SHA-256 fingerprint"
    )
    parser.add_argument(
        "--attempt",
        type=Path,
        help="Preselected new evidence directory for a disposable clean job",
    )
    options = parser.parse_args()
    run(options.candidate, options.lab, options.base, attempt=options.attempt)


if __name__ == "__main__":
    main()
