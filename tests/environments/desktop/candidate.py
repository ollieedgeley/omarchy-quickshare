"""Package native and plugin inputs for identified local verification."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

from __future__ import annotations

import argparse
import fcntl
import json
import os
import shutil
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Explicit owned package/export argv only.
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import build_native
import candidate_contract
import deploy_native
import deploy_panel
import vm_control

CANDIDATES = vm_control.CACHE / "candidates"
SCHEMA_VERSION = 2
PACKAGE = """
import { createArchPackage } from './tools/release/arch-package.mjs';
import { run } from './tools/gates/lib/process.mjs';
const options = JSON.parse(process.argv[1]);
options.runCommand = (command, args) => run(command, args, { quiet: true });
console.log(JSON.stringify(createArchPackage(options)));
"""
EXPORT = """
import { createPluginRepository } from './tools/release/plugin-export.mjs';
createPluginRepository(JSON.parse(process.argv[1]));
"""
PLUGIN_INVENTORY = """
import { PLUGIN_FILES } from './tools/release/plugin-export.mjs';
console.log(JSON.stringify(PLUGIN_FILES));
"""


def now() -> str:
    """Return current UTC timestamp in ISO format."""
    return datetime.now(UTC).isoformat()


def command(args: list[str | Path], directory: Path, log: Path) -> str:
    """Run argv under directory, append all output (incl. failures) to log.

    Returns:
        Stripped stdout on success.

    Raises:
        subprocess.TimeoutExpired: If the command exceeds 120s.
    """
    with log.open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(list(map(str, args))) + "\n")
        stream.flush()
        try:
            result = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Explicit owned package/export argv only.
                list(map(str, args)),
                cwd=directory,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                timeout=120,
                check=False,
            )
        except subprocess.TimeoutExpired as error:
            partial = error.stdout or ""
            stream.write(
                partial.decode(errors="replace")
                if isinstance(partial, bytes)
                else partial
            )
            stream.write("\nCommand timed out after 120 seconds\n")
            raise
        stream.write(result.stdout)
        result.check_returncode()
        return result.stdout.strip()


def inventory(directory: Path) -> dict[str, str]:
    """Return regular-file hashes.

    Raises:
        RuntimeError: A plugin entry is not a regular file.
    """
    files = {}
    for path in sorted(directory.iterdir()):
        if path.name == ".git":
            continue
        if path.is_symlink() or not path.is_file():
            message = f"Plugin input must be a regular file: {path}"
            raise RuntimeError(message)
        files[path.name] = build_native.digest(path)
    return files


def _plugin_git(source: Path, files: dict[str, str], log: Path) -> str:
    expected = json.loads(
        command(
            ["node", "--input-type=module", "-e", PLUGIN_INVENTORY],
            vm_control.ROOT,
            log,
        )
    )
    if set(files) != set(expected):
        message = "Selected plugin differs from the closed export inventory"
        raise RuntimeError(message)
    root = command(["git", "rev-parse", "--show-toplevel"], source, log)
    if Path(root).resolve() != source or command(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        source,
        log,
    ):
        message = "Selected plugin must be a clean standalone Git repository"
        raise RuntimeError(message)
    tree = command(["git", "ls-tree", "-r", "HEAD"], source, log)
    committed = {}
    for row in tree.splitlines():
        entry, name = row.split("\t", 1)
        mode, kind, digest = entry.split()
        if mode not in {"100644", "100755"} or kind != "blob":
            message = "Selected plugin commit contains non-regular entries"
            raise RuntimeError(message)
        committed[name] = digest
    names = sorted(files)
    actual = command(
        ["git", "hash-object", "--no-filters", "--", *names], source, log
    ).splitlines()
    if committed != dict(zip(names, actual, strict=True)):
        message = "Selected plugin files differ from their Git commit"
        raise RuntimeError(message)
    return command(["git", "rev-parse", "HEAD"], source, log)


def _matching_plugin_release(
    source: Path, report: dict[str, Any]
) -> dict[str, Any]:
    metadata = report["nativeBuild"]["metadata"]
    release = json.loads((source / "release.json").read_text(encoding="utf-8"))
    native = release["nativeArtifact"]
    components = {
        role: {"file": item["file"], "sha256": item["sha256"]}
        for role, item in metadata["components"].items()
    }
    if (
        release["sourceCommit"] != metadata["sourceCommit"]
        or native["version"] != report["package"]["version"]
        or native["components"] != components
    ):
        message = "Selected plugin release does not match the native build"
        raise RuntimeError(message)
    candidate_contract.matching_protocol(
        metadata["controlProtocol"], release["controlProtocol"]
    )
    return release


def _copy_plugin(
    source: Path, report: dict[str, Any], log: Path
) -> tuple[dict[str, str], str, bool]:
    repository = Path(report["directory"]) / "plugin"
    if (
        source.is_symlink()
        or not source.is_dir()
        or (source / ".git").is_symlink()
    ):
        message = "Selected plugin must be a regular repository directory"
        raise RuntimeError(message)
    source = source.resolve(strict=True)
    files = inventory(source)
    commit = _plugin_git(source, files, log)
    metadata = report["nativeBuild"]["metadata"]
    selected = metadata["plugin"]
    if (
        Path(selected["directory"]).resolve() != source
        or selected["hashes"] != files
        or selected["commit"] != commit
    ):
        message = "Selected plugin differs from the native build provenance"
        raise RuntimeError(message)
    release = _matching_plugin_release(source, report)
    command(
        ["git", "clone", "--no-local", "--", source, repository],
        vm_control.ROOT,
        log,
    )
    if (
        inventory(repository) != files
        or _plugin_git(repository, files, log) != commit
    ):
        message = "Selected plugin changed during candidate copy"
        raise RuntimeError(message)
    return files, release["sourceCommit"], True


def _create_plugin_repository(report: dict[str, Any], log: Path) -> None:
    source = deploy_panel.SOURCE
    repository = Path(report["directory"]) / "plugin"
    command(
        [
            "node",
            "--input-type=module",
            "-e",
            EXPORT,
            json.dumps({
                "destination": str(repository),
                "source": str(source),
                "sourceCommit": report["sourceHead"],
                "artifacts": {"nativeVersion": report["package"]["version"]},
            }),
        ],
        vm_control.ROOT,
        log,
    )


def _export_plugin(
    report: dict[str, Any], log: Path
) -> tuple[dict[str, str], str, bool]:
    source = deploy_panel.SOURCE
    repository = Path(report["directory"]) / "plugin"
    before = {
        p.name: build_native.digest(p)
        for p in source.iterdir()
        if p.is_file() and not p.is_symlink()
    }
    _create_plugin_repository(report, log)
    files = inventory(repository)
    source_files = {name: build_native.digest(source / name) for name in files}
    if any(source_files[name] != before.get(name) for name in files):
        message = "Plugin sources changed during export"
        raise RuntimeError(message)
    if any(
        files[name] != source_files[name]
        for name in files
        if name != "release.json"
    ):
        message = "Plugin export differs from selected source files"
        raise RuntimeError(message)
    exact = not bool(
        command(
            [
                "git",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--",
                source,
            ],
            vm_control.ROOT,
            log,
        )
    )
    return source_files, report["sourceHead"], exact


def _prepare_native(
    selected: Path | None, plugin: Path | None, report: dict[str, Any]
) -> None:
    log = Path(report["directory"]) / "build.log"
    artifact, metadata = deploy_native._artifact(selected, component="all")  # ruff: ignore[private-member-access] - Internal artifact identity helper
    report["nativeBuild"] = {
        "artifact": str(artifact),
        "metadata": metadata,
    }
    if plugin:
        for path in [
            "LICENSE",
            "packaging/systemd/omarchy-quickshare.toml",
            "crates/app/Cargo.toml",
            "crates/cli/Cargo.toml",
        ]:
            if (
                build_native.digest(vm_control.ROOT / path)
                != metadata["source"]["inputs"][path]
            ):
                message = (
                    f"Packaging input differs from native snapshot: {path}"
                )
                raise RuntimeError(message)
    report["sourceHead"] = (
        metadata["sourceCommit"]
        if plugin
        else command(["git", "rev-parse", "HEAD"], vm_control.ROOT, log)
    )
    report["worktreeStatus"] = (
        ""
        if plugin
        else command(
            ["git", "status", "--porcelain=v1", "--untracked-files=all"],
            vm_control.ROOT,
            log,
        )
    )


def _copy_native_inputs(report: dict[str, Any]) -> None:
    attempt = Path(report["directory"])
    artifact = Path(report["nativeBuild"]["artifact"])
    metadata = report["nativeBuild"]["metadata"]
    inputs = attempt / "native-inputs"
    inputs.mkdir()
    sources = {
        **{
            component["file"]: artifact / component["file"]
            for component in metadata["components"].values()
        },
        build_native.UNIT.name: artifact / build_native.UNIT.name,
        build_native.NAME + ".toml": vm_control.ROOT
        / "packaging/systemd/omarchy-quickshare.toml",
        "LICENSE": vm_control.ROOT / "LICENSE",
    }
    report["nativeInputs"] = {}
    for name, source in sources.items():
        if source.is_symlink() or not source.is_file():
            message = f"Candidate input must be a regular file: {source}"
            raise RuntimeError(message)
        digest = build_native.digest(source)
        shutil.copyfile(source, inputs / name)
        if build_native.digest(inputs / name) != digest:
            message = f"Candidate input changed during copy: {source}"
            raise RuntimeError(message)
        report["nativeInputs"][name] = {
            "source": str(source),
            "sha256": digest,
        }
    candidate_contract.matching_inputs(report)


def _build_package(report: dict[str, Any], log: Path) -> None:
    attempt = Path(report["directory"])
    inputs = attempt / "native-inputs"
    metadata = report["nativeBuild"]["metadata"]
    package = json.loads(
        command(
            [
                "node",
                "--input-type=module",
                "-e",
                PACKAGE,
                json.dumps({
                    "nativeDirectory": str(inputs),
                    "destination": str(attempt / "arch"),
                    "root": str(vm_control.ROOT),
                }),
            ],
            vm_control.ROOT,
            log,
        )
    )
    candidate_contract.matching_version(metadata, package["version"])
    package_path = Path(package["artifact"])
    pkginfo = command(
        ["tar", "--zstd", "-xOf", package_path, ".PKGINFO"],
        vm_control.ROOT,
        log,
    )
    identity = dict(
        line.split(" = ", 1) for line in pkginfo.splitlines() if " = " in line
    )
    report["package"] = {
        "path": str(package_path),
        "sha256": build_native.digest(package_path),
        "version": package["version"],
        "pkginfo": pkginfo,
        "name": identity["pkgname"],
        "packageVersion": identity["pkgver"],
        "architecture": identity["arch"],
        "unitSha256": build_native.digest(
            attempt / "arch" / build_native.UNIT.name
        ),
    }


def _bundle_plugin(
    report: dict[str, Any], plugin: Path | None, log: Path
) -> None:
    attempt = Path(report["directory"])
    metadata = report["nativeBuild"]["metadata"]
    source = plugin.absolute() if plugin else deploy_panel.SOURCE
    repository = attempt / "plugin"
    source_files, source_head, exact = (
        _copy_plugin(source, report, log)
        if plugin
        else _export_plugin(report, log)
    )
    files = inventory(repository)
    manifest = json.loads(
        (repository / "manifest.json").read_text(encoding="utf-8")
    )
    compatibility = json.loads(
        (repository / "release.json").read_text(encoding="utf-8")
    )["controlProtocol"]
    candidate_contract.matching_protocol(
        metadata["controlProtocol"], compatibility
    )
    entry = manifest["entryPoints"]["barWidget"]
    if deploy_panel.STAMP not in (repository / entry).read_text(
        encoding="utf-8"
    ):
        message = "Fresh candidate requires shipped unstamped identity"
        raise RuntimeError(message)
    commit = command(["git", "rev-parse", "HEAD"], repository, log)
    bundle = attempt / "plugin.bundle"
    command(["git", "bundle", "create", bundle, "--all"], repository, log)
    report["plugin"] = {
        "id": manifest["id"],
        "entryPoint": entry,
        "controlProtocol": compatibility,
        "revision": "unversioned",
        "source": str(source),
        "sourceHead": source_head,
        "sourceFiles": source_files,
        "sourceCommitIsExact": exact,
        "repository": str(repository),
        "commit": commit,
        "files": files,
        "bundle": str(bundle),
        "bundleSha256": build_native.digest(bundle),
    }


def build(selected: Path | None, plugin: Path | None = None) -> None:
    """Build candidate package+plugin bundle; retain evidence on failure.

    Preparation and validation errors propagate after failure recording.

    Raises:
        KeyboardInterrupt: Recorded then re-raised.
    """
    attempt = CANDIDATES / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    report = {
        "schemaVersion": SCHEMA_VERSION,
        "candidateId": attempt.name,
        "purpose": "local-verification-not-published-release",
        "startedAt": now(),
        "status": "failed",
        "directory": str(attempt),
    }
    log = attempt / "build.log"
    try:
        _prepare_native(selected, plugin, report)
        _copy_native_inputs(report)
        _build_package(report, log)
        _bundle_plugin(report, plugin, log)
        report["status"] = "built"
    except (Exception, KeyboardInterrupt) as error:
        report["error"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        report["finishedAt"] = now()
        vm_control.save(attempt / "candidate.json", report)
        print(f"Candidate and evidence retained: {attempt}")
    print(json.dumps(report, indent=2))


def load(directory: Path) -> dict[str, Any]:
    """Verify retained candidate inputs without rebuild/Cargo consult.

    Returns:
        The loaded candidate report dict.

    Raises:
        RuntimeError: On schema mismatch, escape, or checksum failure.
    """
    directory = directory.resolve(strict=True)
    report = json.loads(
        (directory / "candidate.json").read_text(encoding="utf-8")
    )
    if (
        report.get("schemaVersion") != SCHEMA_VERSION
        or report.get("status") != "built"
    ):
        message = "Candidate has no successful supported build manifest"
        raise RuntimeError(message)
    metadata = report["nativeBuild"]["metadata"]
    candidate_contract.matching_version(metadata, report["package"]["version"])
    candidate_contract.matching_inputs(report)
    candidate_contract.matching_protocol(
        metadata["controlProtocol"], report["plugin"]["controlProtocol"]
    )
    for path, expected in [
        (report["package"]["path"], report["package"]["sha256"]),
        (report["plugin"]["bundle"], report["plugin"]["bundleSha256"]),
        *[
            (directory / "native-inputs" / name, item["sha256"])
            for name, item in report["nativeInputs"].items()
        ],
    ]:
        selected = Path(path)
        if (
            selected.is_symlink()
            or not selected.is_file()
            or not selected.resolve(strict=True).is_relative_to(directory)
        ):
            message = "Candidate artifact escapes its private directory"
            raise RuntimeError(message)
        if build_native.digest(selected) != expected:
            message = f"Candidate checksum mismatch: {selected}"
            raise RuntimeError(message)
    return report


def main() -> None:
    """Lock builds dir then invoke build on optional selected artifact."""
    os.umask(0o077)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--artifact",
        type=Path,
        help="Selected native build; default latest successful build",
    )
    parser.add_argument(
        "--plugin",
        type=Path,
        help="Exact exported plugin Git repository from the native build",
    )
    options = parser.parse_args()
    build_native.BUILDS.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(
        build_native.BUILDS / "artifacts.lock",
        os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
        0o600,
    )
    with os.fdopen(descriptor, "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_SH | fcntl.LOCK_NB)
        build(options.artifact, options.plugin)


if __name__ == "__main__":
    main()
