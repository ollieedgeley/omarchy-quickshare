"""Read-only probes of a fresh distribution installation."""  # ruff: ignore[implicit-namespace-package] - Standalone guest tooling.

from __future__ import annotations

import hashlib
import json
import subprocess  # ruff: ignore[suspicious-subprocess-import]  # subprocess module justified for bounded guest probe helper
import sys
from pathlib import Path
from typing import Any

NATIVE_SCHEMA_VERSION = 2


def output(*args: str) -> str:
    """Execute trusted command via subprocess and return stripped output.

    Returns:
        Stripped text output from command.
    """
    return subprocess.check_output(args, text=True, timeout=10).strip()  # ruff: ignore[subprocess-without-shell-equals-true]  # bounded trusted args only


def digest(path: str | Path) -> str:
    """Return SHA-256 hex digest of the file contents at path.

    Returns:
        Hexadecimal digest string.
    """
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def absent(plugin_id: str) -> dict[str, Any]:
    """Verify candidate and related data are absent from the fresh guest.

    Returns:
        Status dict with candidateAbsent, catalogs, unit state, home.

    Raises:
        RuntimeError: Candidate files, registry entries or a unit already exist.
    """
    home = Path.home()
    paths: list[Path] = [
        home / ".local/bin/omarchy-quickshare",
        Path("/usr/bin/omarchy-quickshare"),
        home / ".local/bin/omarchy-quickshare-daemon",
        Path("/usr/bin/omarchy-quickshare-daemon"),
        home / ".config/systemd/user/omarchy-quickshare.service",
        Path("/usr/lib/systemd/user/omarchy-quickshare.service"),
        home / ".config/omarchy/plugins" / plugin_id,
    ]
    paths.extend(
        home / directory / "omarchy-quickshare"
        for directory in [".config", ".cache", ".local/share", ".local/state"]
    )
    for path in paths:
        if path.exists() or path.is_symlink():
            msg = f"Clean base contains candidate or retained data: {path}"
            raise RuntimeError(msg)
    packages = output("pacman", "-Qq").splitlines()
    if "omarchy-quickshare" in packages:
        msg = "Candidate package already installed"
        raise RuntimeError(msg)
    catalog = json.loads(output("omarchy-plugin-catalog"))
    plugins = json.loads(output("omarchy-shell", "shell", "listPlugins"))
    if any(item["id"] == plugin_id for item in catalog + plugins):
        msg = "Candidate plugin already installed or registered"
        raise RuntimeError(msg)
    unit = output(
        "systemctl",
        "--user",
        "show",
        "omarchy-quickshare.service",
        "-p",
        "LoadState",
        "--value",
    )
    if unit != "not-found":
        msg = f"Candidate unit is not absent: {unit}"
        raise RuntimeError(msg)
    return {
        "candidateAbsent": True,
        "pluginCatalog": catalog,
        "registeredPlugins": plugins,
        "unitLoadState": unit,
        "home": str(home),
    }


def _unit_identity(unit: str) -> tuple[str, str]:
    pid = output(
        "systemctl", "--user", "show", unit, "-p", "MainPID", "--value"
    )
    if int(pid) <= 0:
        message = "Packaged daemon has no main PID"
        raise RuntimeError(message)
    fragment = output(
        "systemctl", "--user", "show", unit, "-p", "FragmentPath", "--value"
    )
    if fragment != "/usr/lib/systemd/user/" + unit:
        message = f"Unit is not the shipped package unit: {fragment}"
        raise RuntimeError(message)
    dropins = output(
        "systemctl", "--user", "show", unit, "-p", "DropInPaths", "--value"
    )
    if dropins:
        message = f"Candidate has unit overrides: {dropins}"
        raise RuntimeError(message)
    return pid, fragment


def _installed_components(candidate: dict[str, Any]) -> dict[str, Any]:
    metadata = candidate["nativeBuild"]["metadata"]
    expected_protocol = metadata.get("controlProtocol")
    if (
        metadata.get("schemaVersion") != NATIVE_SCHEMA_VERSION
        or type(expected_protocol) is not int
        or expected_protocol <= 0
        or set(metadata.get("components", {})) != {"cli", "daemon"}
    ):
        message = "Candidate requires both components and a valid protocol"
        raise RuntimeError(message)
    components = {}
    for role, name, argument in [
        ("cli", "omarchy-quickshare", "protocol-version"),
        ("daemon", "omarchy-quickshare-daemon", "--protocol-version"),
    ]:
        binary = Path("/usr/bin") / name
        expected = metadata["components"][role]
        if binary.is_symlink() or not binary.is_file():
            message = f"Installed {role} must be a regular executable: {binary}"
            raise RuntimeError(message)
        installed = digest(binary)
        if expected["file"] != name or installed != expected["sha256"]:
            message = f"Installed {role} checksum differs from candidate"
            raise RuntimeError(message)
        protocol = output(str(binary), argument)
        if protocol != str(expected_protocol):
            message = f"Installed {role} control protocol mismatch: {protocol}"
            raise RuntimeError(message)
        components[role] = {
            "executable": str(binary),
            "installedSha256": installed,
            "controlProtocol": expected_protocol,
        }
    return components


def native(candidate: dict[str, Any]) -> dict[str, Any]:
    """Verify packaged native files, loaded daemon and public interfaces.

    Returns:
        Native file, process, unit, package and public status identities.

    Raises:
        RuntimeError: Native identity, checksum, state or readiness differs.
    """
    name = "omarchy-quickshare"
    unit = name + ".service"
    components = _installed_components(candidate)
    binary = components["daemon"]["executable"]
    pid, fragment = _unit_identity(unit)
    executable = str(Path(f"/proc/{pid}/exe").readlink())
    hashes = {
        "installed": components["daemon"]["installedSha256"],
        "loaded": digest(f"/proc/{pid}/exe"),
    }
    if executable != binary or hashes["loaded"] != hashes["installed"]:
        msg = f"Loaded daemon identity mismatch: {executable}, {hashes}"
        raise RuntimeError(msg)
    if digest(fragment) != candidate["package"]["unitSha256"]:
        msg = "Installed shipped unit checksum mismatch"
        raise RuntimeError(msg)
    package = output("pacman", "-Q", name)
    expected_pkg = (
        candidate["package"]["name"]
        + " "
        + candidate["package"]["packageVersion"]
    )
    if package != expected_pkg:
        msg = f"Installed package identity mismatch: {package}"
        raise RuntimeError(msg)
    cli = components["cli"]["executable"]
    health = output(cli, "health")
    status = json.loads(output(cli, "status", "--json"))
    if health != "available" or status["response"]["type"] != "snapshot":
        msg = "Candidate public interfaces are unavailable"
        raise RuntimeError(msg)
    if output("systemctl", "--user", "is-active", unit) != "active":
        msg = "Candidate unit is inactive"
        raise RuntimeError(msg)
    if (
        output("systemctl", "--user", "show", unit, "-p", "MainPID", "--value")
        != pid
    ):
        msg = "Candidate daemon changed during readiness probe"
        raise RuntimeError(msg)
    components["daemon"]["loadedSha256"] = hashes["loaded"]
    return {
        "mainPid": int(pid),
        "executable": executable,
        "hashes": hashes,
        "components": components,
        "installedSha256": hashes["installed"],
        "loadedSha256": hashes["loaded"],
        "effectiveUnitPath": fragment,
        "unitSha256": digest(fragment),
        "package": package,
        "health": health,
        "publicStatus": status,
    }


def _plugin_inventory(root: Path) -> dict[str, str]:
    if root.is_symlink() or not (root / ".git").is_dir():
        message = "Plugin is not a distribution-installed Git checkout"
        raise RuntimeError(message)
    files = {}
    for path in sorted(root.iterdir()):
        if path.name == ".git":
            continue
        if path.is_symlink() or not path.is_file():
            message = f"Unexpected installed plugin entry: {path}"
            raise RuntimeError(message)
        files[path.name] = digest(path)
    return files


def plugin(candidate: dict[str, Any], nonce: str) -> dict[str, Any]:
    """Verify installed plugin bytes, commit and live registration.

    Returns:
        Installed directory, inventory, commit and live component identity.

    Raises:
        RuntimeError: Checkout, inventory, commit or live identity differs.
    """
    expected = candidate["plugin"]
    root = Path.home() / ".config/omarchy/plugins" / expected["id"]
    files = _plugin_inventory(root)
    if files != expected["files"]:
        msg = "Installed plugin inventory differs from candidate"
        raise RuntimeError(msg)
    commit = output("git", "-C", str(root), "rev-parse", "HEAD")
    if commit != expected["commit"]:
        msg = "Installed plugin Git commit differs from candidate"
        raise RuntimeError(msg)
    if output(
        "git",
        "-C",
        str(root),
        "status",
        "--porcelain",
        "--untracked-files=all",
    ):
        msg = "Installed plugin checkout is dirty"
        raise RuntimeError(msg)
    plugins = json.loads(output("omarchy-shell", "shell", "listPlugins"))
    registered = next(
        (item for item in plugins if item["id"] == expected["id"]), None
    )
    if not registered or not registered["enabled"]:
        msg = "Distribution installer has not enabled the plugin"
        raise RuntimeError(msg)
    identity = json.loads(
        output("omarchy-shell", expected["id"], "deploymentIdentity", nonce)
    )
    wanted = {
        "id": expected["id"],
        "revision": expected["revision"],
        "component": (root / expected["entryPoint"]).as_uri(),
        "nonce": nonce,
    }
    if identity != wanted:
        msg = f"Fresh live plugin identity mismatch: {identity}"
        raise RuntimeError(msg)
    return {
        "installedDirectory": str(root),
        "files": files,
        "commit": commit,
        "loadedIdentity": identity,
        "registryIdentity": registered,
    }


if __name__ == "__main__":
    manifest = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
    if sys.argv[1] == "absent":
        result = absent(manifest["plugin"]["id"])
    elif sys.argv[1] == "native":
        result = native(manifest)
    elif sys.argv[1] == "plugin":
        result = plugin(manifest, sys.argv[3])
    else:
        message = "Unknown fresh-install probe"
        raise RuntimeError(message)
    print(json.dumps(result))
