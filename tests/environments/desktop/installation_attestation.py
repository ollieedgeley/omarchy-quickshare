"""Re-attest a resumed owned installation without rewriting its history."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import hashlib
import json
import shlex
import stat
import uuid
from pathlib import Path
from typing import Any

import clean_install
import lab_lifecycle
import vm_control
from scenario_state import now


def _identity(path: Path) -> dict[str, int]:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode):
        message = f"Attestation needs a regular owned file: {path}"
        raise RuntimeError(message)
    return {"device": info.st_dev, "inode": info.st_ino}


def _owner_continuity(state: dict, installation: dict) -> None:
    owner = Path(installation["ownerDirectory"])
    identity = owner.lstat()
    retained = lab_lifecycle.active(owner)
    if (
        not stat.S_ISDIR(identity.st_mode)
        or not installation.get("createdOwner")
        or installation["ownerIdentity"]
        != {
            "device": identity.st_dev,
            "inode": identity.st_ino,
        }
    ):
        message = "Original installation owner or live guest ownership changed"
        raise RuntimeError(message)
    if (
        not retained
        or any(state.get(key) != value for key, value in retained.items())
        or not vm_control.running(state)
    ):
        message = "Live guest differs from the retained ownership record"
        raise RuntimeError(message)


def _input_continuity(state: dict, installation: dict) -> Path:
    if (
        any(
            state.get(key) != installation["guest"].get(key)
            for key in (
                "uuid",
                "directory",
                "stateFile",
                "disk",
                "firmware",
                "firmwareCode",
                "sshKey",
                "fingerprint",
            )
        )
        or state["fingerprint"] != installation["baseFingerprint"]
    ):
        message = "Resumed guest does not use the original owned storage inputs"
        raise RuntimeError(message)
    base = Path(installation["baseDirectory"])
    owner = Path(installation["ownerDirectory"])
    if (owner / "known_hosts").read_bytes() != (
        base / "known_hosts"
    ).read_bytes():
        message = "Retained SSH host pin differs from the installed base"
        raise RuntimeError(message)
    return base


def _opened_images(state: dict) -> dict:
    files = {key: _identity(Path(state[key])) for key in ("disk", "firmware")}
    opened = {}
    for descriptor in Path(f"/proc/{state['pid']}/fd").iterdir():
        try:
            target = str(descriptor.readlink())
            info = descriptor.stat()
        except FileNotFoundError:
            continue
        opened.update(
            (key, expected)
            for key, expected in files.items()
            if target == state[key]
            and expected
            == {
                "device": info.st_dev,
                "inode": info.st_ino,
            }
        )
    if opened != files:
        message = "Live QEMU is not holding the recorded owned image files"
        raise RuntimeError(message)
    return files


def _backing_file(state: dict, base: Path) -> str:
    blocks = vm_control.qmp(state, "query-block")
    matches = [
        block["inserted"]
        for block in blocks
        if block.get("inserted", {}).get("file") == state["disk"]
    ]
    if len(matches) != 1:
        message = "Cannot identify one live QEMU system disk"
        raise RuntimeError(message)
    disk = matches[0]
    image = disk["image"]
    backing = image.get("full-backing-filename") or image.get(
        "backing-filename"
    )
    if (
        disk.get("ro") is not False
        or image.get("format") != "qcow2"
        or image.get("filename") != state["disk"]
        or backing != str(base / "base.qcow2")
    ):
        message = "Live writable system disk has different backing storage"
        raise RuntimeError(message)
    return backing


def _storage(state: dict, installation: dict) -> dict:
    _owner_continuity(state, installation)
    base = _input_continuity(state, installation)
    files = _opened_images(state)
    backing = _backing_file(state, base)
    return {
        "uuid": state["uuid"],
        "qemuPid": state["pid"],
        "qemuStart": Path(f"/proc/{state['pid']}/stat")
        .read_text(encoding="utf-8")
        .rsplit(")", 1)[1]
        .split()[19],
        "ownerIdentity": installation["ownerIdentity"],
        "files": files,
        "disk": state["disk"],
        "backingFile": backing,
    }


def _evidence(state: dict, installation: dict, path: Path) -> tuple:
    original = path.read_bytes()
    if (
        json.loads(original) != installation
        or installation.get("status") != "ready"
    ):
        message = "Installation manifest changed before re-attestation"
        raise RuntimeError(message)
    return original, {
        "status": "failed",
        "startedAt": now(),
        "installationManifest": str(path.resolve()),
        "installationManifestSha256": hashlib.sha256(original).hexdigest(),
        "originalGuest": installation["guest"],
        "guest": {
            key: state[key]
            for key in (
                "uuid",
                "pid",
                "directory",
                "disk",
                "firmware",
                "fingerprint",
            )
        },
        "storageContinuity": "original owner inode, UUID, paths and backing",
        "originalWritableImageInodeRecorded": False,
    }


def _probe_inputs(state: dict, installation: dict, retained: tuple) -> None:
    attempt, evidence = retained
    stage = Path(installation["guestStage"])
    evidence["probeInputs"] = {}
    for name, expected in (
        ("candidate.json", installation["candidateManifestSha256"]),
        (
            clean_install.PROBE.name,
            hashlib.sha256(clean_install.PROBE.read_bytes()).hexdigest(),
        ),
    ):
        actual = clean_install.guest(
            state,
            shlex.join(["sha256sum", str(stage / name)]),
            attempt,
            "reattest-inputs",
        ).split()[0]
        if actual != expected:
            message = f"Retained installed-probe input differs: {name}"
            raise RuntimeError(message)
        evidence["probeInputs"][name] = actual


def reattest(  # ruff: ignore[too-many-arguments, too-many-positional-arguments] - The original record and new retained attempt must remain distinct.
    state: dict[str, Any],
    installation: dict[str, Any],
    path: Path,
    attempt: Path,
) -> dict[str, Any]:
    """Write fresh runtime evidence linked to the immutable installation.

    Returns:
        Successful owner, storage and installed-candidate attestation.

    Raises:
        RuntimeError: Original record, storage or installed bytes differ.
        KeyboardInterrupt: The operator interrupts the retained probe.
    """
    original, evidence = _evidence(state, installation, path)
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Retain all partial re-attestation evidence on any probe failure.
        evidence["storage"] = _storage(state, installation)
        _probe_inputs(state, installation, (attempt, evidence))
        evidence["native"] = clean_install.probe(
            state, installation, attempt, "reattest-native", "native"
        )
        evidence["plugin"] = clean_install.probe(
            state,
            installation,
            attempt,
            "reattest-plugin",
            "plugin",
            str(uuid.uuid4()),
        )
        if (
            _storage(state, installation) != evidence["storage"]
            or path.read_bytes() != original
        ):
            message = "Storage or manifest changed during re-attestation"
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Outer handler records the failed attestation.
        evidence["status"] = "ready"
    except (Exception, KeyboardInterrupt) as error:
        evidence["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        evidence["finishedAt"] = now()
        vm_control.save(attempt / "installation-attestation.json", evidence)
    return evidence
