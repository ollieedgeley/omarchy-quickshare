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
import deploy_native
import deploy_panel
import vm_control

CANDIDATES = vm_control.CACHE / "candidates"
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
    """Return sha256 map of regular non-.git files under directory."""
    return {
        path.name: build_native.digest(path)
        for path in sorted(directory.iterdir())
        if path.name != ".git"
    }


def _matching_version(metadata: dict[str, Any], version: str) -> None:
    package_id = metadata["cargoArtifact"]["packageId"]
    if not package_id.endswith(f"#{build_native.NAME}@{version}"):
        message = (
            f"Package version {version} differs from the selected native "
            f"build identity {package_id}; select a matching build"
        )
        raise RuntimeError(message)


def build(selected: Path | None) -> None:  # ruff: ignore[too-many-branches,too-many-statements,too-many-locals] - One retained report spans source identities, package creation and plugin export.
    """Build candidate package+plugin bundle; retain evidence on failure.

    Raises:
        RuntimeError: Validation failures (recorded then re-raised).
        KeyboardInterrupt: Recorded then re-raised.
    """
    attempt = CANDIDATES / str(uuid.uuid4())
    attempt.mkdir(parents=True)
    report = {
        "schemaVersion": 1,
        "candidateId": attempt.name,
        "purpose": "local-verification-not-published-release",
        "startedAt": now(),
        "status": "failed",
        "directory": str(attempt),
    }
    log = attempt / "build.log"
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Preserve one report across independent preparation/export steps for failure retention
        artifact, metadata = deploy_native._artifact(selected)  # ruff: ignore[private-member-access] - Internal artifact identity helper
        report["nativeBuild"] = {
            "artifact": str(artifact),
            "metadata": metadata,
        }
        report["sourceHead"] = command(
            ["git", "rev-parse", "HEAD"], vm_control.ROOT, log
        )
        report["worktreeStatus"] = command(
            ["git", "status", "--porcelain=v1", "--untracked-files=all"],
            vm_control.ROOT,
            log,
        )
        inputs = attempt / "native-inputs"
        inputs.mkdir()
        sources = {
            build_native.NAME: artifact / build_native.NAME,
            build_native.UNIT.name: artifact / build_native.UNIT.name,
            build_native.NAME + ".toml": vm_control.ROOT
            / "packaging/systemd/omarchy-quickshare.toml",
            "LICENSE": vm_control.ROOT / "LICENSE",
        }
        report["nativeInputs"] = {}
        for name, source in sources.items():
            if source.is_symlink() or not source.is_file():
                message = f"Candidate input must be a regular file: {source}"
                raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
            digest = build_native.digest(source)
            shutil.copyfile(source, inputs / name)
            if build_native.digest(inputs / name) != digest:
                message = f"Candidate input changed during copy: {source}"
                raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
            report["nativeInputs"][name] = {
                "source": str(source),
                "sha256": digest,
            }
        if (
            report["nativeInputs"][build_native.NAME]["sha256"]
            != metadata["sha256"]
        ):
            message = "Native artifact changed during candidate creation"
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
        if (
            report["nativeInputs"][build_native.UNIT.name]["sha256"]
            != metadata["unitSha256"]
        ):
            message = "Native unit changed during candidate creation"
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
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
        _matching_version(metadata, package["version"])
        package_path = Path(package["artifact"])
        pkginfo = command(
            ["tar", "--zstd", "-xOf", package_path, ".PKGINFO"],
            vm_control.ROOT,
            log,
        )
        identity = dict(
            line.split(" = ", 1)
            for line in pkginfo.splitlines()
            if " = " in line
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
        # The exporter alone owns its closed inventory. Hash source files before
        # invoking it, then select only names it actually exported.
        source = deploy_panel.SOURCE
        before = {
            p.name: build_native.digest(p)
            for p in source.iterdir()
            if p.is_file() and not p.is_symlink()
        }
        repository = attempt / "plugin"
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
                    "artifacts": {"nativeVersion": package["version"]},
                }),
            ],
            vm_control.ROOT,
            log,
        )
        files = inventory(repository)
        source_files = {
            name: build_native.digest(source / name) for name in files
        }
        if any(source_files[name] != before.get(name) for name in files):
            message = "Plugin sources changed during export"
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
        if any(
            files[name] != source_files[name]
            for name in files
            if name != "release.json"
        ):
            message = "Plugin export differs from selected source files"
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
        manifest = json.loads(
            (repository / "manifest.json").read_text(encoding="utf-8")
        )
        entry = manifest["entryPoints"]["barWidget"]
        if deploy_panel.STAMP not in (repository / entry).read_text(
            encoding="utf-8"
        ):
            message = "Fresh candidate requires shipped unstamped identity"
            raise RuntimeError(message)  # ruff: ignore[raise-within-try] - Retain exact failure evidence
        commit = command(["git", "rev-parse", "HEAD"], repository, log)
        bundle = attempt / "plugin.bundle"
        command(["git", "bundle", "create", bundle, "--all"], repository, log)
        report["plugin"] = {
            "id": manifest["id"],
            "entryPoint": entry,
            "revision": "unversioned",
            "source": str(source),
            "sourceHead": report["sourceHead"],
            "sourceFiles": source_files,
            "sourceCommitIsExact": not bool(
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
            ),
            "repository": str(repository),
            "commit": commit,
            "files": files,
            "bundle": str(bundle),
            "bundleSha256": build_native.digest(bundle),
        }
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
    if report.get("schemaVersion") != 1 or report.get("status") != "built":
        message = "Candidate has no successful supported build manifest"
        raise RuntimeError(message)
    for path, expected in [
        (report["package"]["path"], report["package"]["sha256"]),
        (report["plugin"]["bundle"], report["plugin"]["bundleSha256"]),
    ]:
        selected = Path(path)
        if selected.is_symlink() or not selected.resolve(
            strict=True
        ).is_relative_to(directory):
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
    options = parser.parse_args()
    build_native.BUILDS.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(
        build_native.BUILDS / "artifacts.lock",
        os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW,
        0o600,
    )
    with os.fdopen(descriptor, "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_SH | fcntl.LOCK_NB)
        build(options.artifact)


if __name__ == "__main__":
    main()
