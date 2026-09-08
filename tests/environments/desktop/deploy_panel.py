"""Deploy QML into one running named lab and verify the live component."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import argparse
import fcntl
import hashlib
import json
import os
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Bounded external tooling.
import sys
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import deploy_native
import lab_lifecycle
import vm_control

ROOT = Path(__file__).resolve().parents[3]
SOURCE = ROOT / "packaging" / "omarchy-plugin"
STAMP = 'readonly property string deploymentRevision: "unversioned"'
EXPORT = """import { exportPlugin } from './tools/release/plugin-export.mjs';
exportPlugin(JSON.parse(process.argv[1]));
"""
INSTALL = """import json, os, shutil, subprocess, sys
from pathlib import Path
stage, plugin_id, attempt = sys.argv[1:]
source = Path(stage) / 'panel'
parent = Path.home() / '.config/omarchy/plugins'
target = parent / plugin_id
candidate = parent / ('.quickshare-candidate-' + attempt)
previous = parent / ('.quickshare-previous-' + attempt)
for path in [parent, *parent.parents, target]:
    if path.is_symlink():
        raise RuntimeError('Refusing symlink plugin destination: ' + str(path))
if target.exists():
    manifest = json.loads((target / 'manifest.json').read_text())
    if manifest['id'] != plugin_id:
        raise RuntimeError('Installed directory belongs to another plugin')
subprocess.run(['omarchy-plugin-validate', str(source)], check=True)
parent.mkdir(parents=True, exist_ok=True)
shutil.copytree(source, candidate)
if target.exists():
    target.rename(previous)
try:
    candidate.rename(target)
except OSError:
    if previous.exists():
        previous.rename(target)
    raise
print(json.dumps({'installedDirectory': str(target),
                  'previousDirectory':
                      str(previous) if previous.exists() else None}))
"""
INSTALLED = """import hashlib, json, sys
from pathlib import Path
root = Path.home() / '.config/omarchy/plugins' / sys.argv[1]
files = {}
for path in sorted(root.iterdir()):
    if path.is_symlink() or not path.is_file():
        raise RuntimeError('Unexpected installed plugin entry: ' + str(path))
    files[path.name] = hashlib.sha256(path.read_bytes()).hexdigest()
print(json.dumps({'manifest': json.loads((root / 'manifest.json').read_text()),
                  'files': files, 'directory': str(root)}))
"""
NATIVE = """set -Eeuo pipefail
systemctl --user is-active --quiet omarchy-quickshare.service
systemctl --user show omarchy-quickshare.service -p MainPID --value
"""
UNLOCKED = """import json, os, subprocess
instances = json.loads(subprocess.check_output(
    ['hyprctl', 'instances', '-j'], text=True))
if len(instances) != 1:
    raise RuntimeError('Cannot identify one guest compositor')
environment = dict(os.environ,
    HYPRLAND_INSTANCE_SIGNATURE=instances[0]['instance'])
compositor = subprocess.run(
    ['omarchy-hyprland-session-locked'], env=environment)
if compositor.returncode != 1:
    raise RuntimeError('Guest compositor is locked or lock state is unknown; '
                       'unlock the desktop before panel deployment')
status = json.loads(subprocess.check_output(
    ['omarchy-shell', 'lock', 'status'], text=True))
fields = ['locked', 'requested', 'pending', 'sessionLocked', 'secure']
if any(status.get(field) is not False for field in fields):
    raise RuntimeError('Guest lock service is not idle; '
                       'unlock the desktop before panel deployment')
print(json.dumps(status))
"""
JOURNAL = """journalctl --user -b -t omarchy-shell -n 250 --no-pager;
systemctl --user status omarchy-quickshare.service --no-pager;
journalctl --user -b -u omarchy-quickshare.service -n 80 --no-pager
"""


def _guest(state: dict[str, Any], script: str, log: Path) -> str:
    return deploy_native._guest(  # ruff: ignore[private-member-access] - Reuse pinned transport and retained logging.
        state,
        "bash -lc " + shlex.quote(deploy_native.SESSION + script),
        log,
    )


def _python(script: str, *args: str) -> str:
    return shlex.join(["python", "-c", script, *args])


def _hashes(source: Path) -> dict[str, str]:
    return {
        path.name: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(source.iterdir())
    }


def _snapshot(selected: Path, attempt: Path) -> dict[str, Any]:
    # The release exporter owns PLUGIN_FILES and regular-file checks. Do not
    # maintain another QML inventory or invoke its release/build entry point.
    commit = vm_control.output(["git", "rev-parse", "HEAD"], cwd=ROOT)
    snapshot = attempt / "panel"
    options = {
        "source": str(selected.resolve()),
        "destination": str(snapshot),
        "sourceCommit": commit,
    }
    with (attempt / "export.log").open("w", encoding="utf-8") as log:
        vm_control.run(
            ["node", "--input-type=module", "-e", EXPORT, json.dumps(options)],
            cwd=ROOT,
            stdout=log,
            stderr=log,
            timeout=60,
        )
    source_hashes = {
        name: hashlib.sha256((selected / name).read_bytes()).hexdigest()
        for name in _hashes(snapshot)
    }
    digest = hashlib.sha256(
        json.dumps(source_hashes, sort_keys=True).encode()
    ).hexdigest()
    manifest = json.loads((snapshot / "manifest.json").read_text())
    shipped = json.loads((SOURCE / "manifest.json").read_text())
    if (
        manifest["id"] != shipped["id"]
        or manifest["entryPoints"] != shipped["entryPoints"]
    ):
        message = "Panel deployment must preserve shipped identity/entry points"
        raise RuntimeError(message)
    entry = snapshot / manifest["entryPoints"]["barWidget"]
    text = entry.read_text()
    if text.count(STAMP) != 1:
        message = "Panel source needs one unstamped deploymentRevision"
        raise RuntimeError(message)
    revision = f"{digest}:{attempt.name}"
    entry.write_text(
        text.replace(
            STAMP,
            "readonly property string deploymentRevision: "
            + json.dumps(revision),
        ),
        encoding="utf-8",
    )
    return {
        "source": str(selected.resolve()),
        "sourceCommit": commit,
        "sourceFiles": source_hashes,
        "sourceSha256": digest,
        "revision": revision,
        "pluginId": manifest["id"],
        "entryPoint": manifest["entryPoints"]["barWidget"],
        "deployedFiles": _hashes(snapshot),
    }


def _install(
    state: dict[str, Any], attempt: Path, plugin_id: str
) -> dict[str, Any]:
    stage = f".cache/omarchy-quickshare-panel/{attempt.name}"
    _guest(state, "mkdir -p -- " + shlex.quote(stage), attempt / "copy.log")
    with (attempt / "copy.log").open("a", encoding="utf-8") as log:
        vm_control.run(
            [
                "scp",
                "-F",
                Path(state["directory"]) / "ssh.config",
                "-r",
                attempt / "panel",
                f"desktop:{stage}/",
            ],
            stdout=log,
            stderr=log,
            timeout=120,
        )
    installed = json.loads(
        _guest(
            state,
            _python(INSTALL, stage, plugin_id, attempt.name),
            attempt / "install.log",
        )
    )
    return {"guestStage": stage, **installed}


def _probe_loaded(
    state: dict[str, Any], report: dict[str, Any], attempt: Path
) -> dict[str, Any]:
    plugin_id = report["pluginId"]
    plugins = json.loads(
        _guest(state, "omarchy-shell shell listPlugins", attempt / "reload.log")
    )
    plugin = next((item for item in plugins if item["id"] == plugin_id), None)
    if plugin is None:
        message = "Host has not registered the installed plugin"
        raise RuntimeError(message)
    if not plugin["enabled"]:
        result = _guest(
            state,
            shlex.join([
                "omarchy-shell",
                "shell",
                "putBarWidget",
                plugin_id,
                "{}",
            ]),
            attempt / "reload.log",
        ).strip()
        message = f"Waiting for enabled panel; host placement returned {result}"
        raise RuntimeError(message)
    nonce = str(uuid.uuid4())
    identity = json.loads(
        _guest(
            state,
            shlex.join([
                "omarchy-shell",
                plugin_id,
                "deploymentIdentity",
                nonce,
            ]),
            attempt / "identity.log",
        )
    )
    expected = {
        "id": plugin_id,
        "revision": report["revision"],
        "component": Path(
            report["installedDirectory"], report["entryPoint"]
        ).as_uri(),
        "nonce": nonce,
    }
    if identity != expected:
        message = f"Loaded component differs from deployed revision: {identity}"
        raise RuntimeError(message)
    return {"loadedIdentity": identity, "registryIdentity": plugin}


def _loaded(
    state: dict[str, Any], report: dict[str, Any], attempt: Path
) -> dict[str, Any]:
    # Registry rescans can keep the old compiled QML alive on the installed
    # runtime. The supported restart command creates a fresh shell engine,
    # preserves the compositor/native service, and refuses an active lock.
    _guest(state, "omarchy-restart-shell", attempt / "reload.log")
    deadline = time.monotonic() + 45
    last = "No runtime identity response"
    while time.monotonic() < deadline:
        try:
            return _probe_loaded(state, report, attempt)
        except (
            ValueError,
            KeyError,
            OSError,
            RuntimeError,
            subprocess.SubprocessError,
        ) as error:
            last = str(error)
        time.sleep(1)
    message = (
        f"Panel reload failed: {last}; see shell-journal.log and identity.log"
    )
    raise RuntimeError(message)


def _deploy(
    state: dict[str, Any], attempt: Path, report: dict[str, Any]
) -> None:
    before = _guest(state, NATIVE, attempt / "native-before.log").strip()
    report["nativePidBefore"] = int(before)
    # The host watcher reloads lock services as soon as installation changes.
    # Refuse locked/indeterminate desktops before touching the plugin tree.
    report["lockPreflight"] = json.loads(
        _guest(state, _python(UNLOCKED), attempt / "lock-preflight.log")
    )
    report.update(_install(state, attempt, report["pluginId"]))
    installed = json.loads(
        _guest(
            state,
            _python(INSTALLED, report["pluginId"]),
            attempt / "installed.log",
        )
    )
    report["installedIdentity"] = installed
    if installed["files"] != report["deployedFiles"]:
        message = "Installed plugin bytes differ from the deployment snapshot"
        raise RuntimeError(message)
    report.update(_loaded(state, report, attempt))
    after = _guest(state, NATIVE, attempt / "native-after.log").strip()
    report["nativePidAfter"] = int(after)
    if before != after:
        message = "Native service changed during a QML-only deployment"
        raise RuntimeError(message)


def _journal(state: dict[str, Any], attempt: Path) -> None:
    try:
        result = vm_control.ssh(
            state,
            "bash -lc " + shlex.quote(deploy_native.SESSION + JOURNAL),
            check=False,
        )
        (attempt / "shell-journal.log").write_text(
            result.stdout + result.stderr, encoding="utf-8"
        )
    except (OSError, subprocess.SubprocessError) as error:
        (attempt / "journal-error.log").write_text(str(error), encoding="utf-8")


def _running(owner: Path) -> dict[str, Any]:
    state = lab_lifecycle.active(owner)
    if not state or not vm_control.running(state):
        message = "Selected lab must be running; deployment never starts a VM"
        raise RuntimeError(message)
    return state


def _attempt(owner: Path, selected: Path) -> None:
    attempt = owner / "panel-deployments" / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    report = {
        "deploymentId": attempt.name,
        "lab": owner.name,
        "startedAt": datetime.now(UTC).isoformat(),
        "status": "failed",
    }
    state = None
    try:
        state = _running(owner)
        report.update({"guestUuid": state["uuid"], "qemuPid": state["pid"]})
        report.update(_snapshot(selected, attempt))
        _deploy(state, attempt, report)
        report["status"] = "ready"
    except (
        OSError,
        ValueError,
        KeyError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        report["error"] = str(error)
        print(
            f"Panel deployment failed; retained lab and evidence: {attempt}",
            file=sys.stderr,
        )
        raise
    finally:
        report["finishedAt"] = datetime.now(UTC).isoformat()
        vm_control.save(attempt / "deployment.json", report)
        if state and vm_control.running(state):
            _journal(state, attempt)
    print(json.dumps(report, indent=2))


def main() -> None:
    """Deploy to DESKTOP_LAB under its operation lock."""
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--source",
        type=Path,
        default=SOURCE,
        help="Plugin source directory; default: packaging/omarchy-plugin",
    )
    options = parser.parse_args()
    owner = lab_lifecycle.directory()
    with (owner / "operation.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        _attempt(owner, options.source)


if __name__ == "__main__":
    main()
