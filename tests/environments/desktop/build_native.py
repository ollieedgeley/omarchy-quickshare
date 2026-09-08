"""Build identified native components without touching a desktop lab."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import fcntl
import hashlib
import json
import os
import shutil
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Catch fixed-argv tool failures.
import sys
import time
import uuid
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import resource_budget
import vm_control

NAME = "omarchy-quickshare"
BUILDS = vm_control.CACHE / "native"
UNIT = vm_control.ROOT / "packaging/systemd" / f"{NAME}.service"
SCHEMA_VERSION = 2
COMPONENTS = {
    "cli": {"file": NAME, "package": "omarchy-quickshare-cli"},
    "daemon": {"file": NAME + "-daemon", "package": NAME},
}


def components(component: str = "all") -> tuple[str, ...]:
    """Return selected roles.

    Raises:
        ValueError: The component selector is unknown.
    """
    if component == "all":
        return tuple(COMPONENTS)
    if component not in COMPONENTS:
        message = "NATIVE_COMPONENT must be all, cli or daemon"
        raise ValueError(message)
    return (component,)


def digest(path: Path) -> str:
    """Return a streaming SHA-256 digest of an artifact.

    Returns:
        The lowercase SHA-256 hexadecimal digest.
    """
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def _cargo_configuration() -> dict[str, str | None]:
    # Cargo also reads configuration outside the workspace.
    cargo_home = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    configurations = {
        directory / ".cargo" / name
        for directory in (vm_control.ROOT, *vm_control.ROOT.parents)
        for name in ("config", "config.toml")
    } | {cargo_home / name for name in ("config", "config.toml")}
    return {
        str(path): digest(path) if path.is_file() else None
        for path in sorted(configurations)
    }


def _source_identity() -> dict[str, Any]:
    paths = vm_control.output(
        [
            "git",
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            "crates",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain",
            "rust-toolchain.toml",
            ".cargo",
            "packaging/systemd",
            "tests/support",
            "tests/suites/contracts",
            "tools/gates",
            "tests/environments/desktop/build_native.py",
        ],
        cwd=vm_control.ROOT,
    ).split("\0")
    inputs = {
        name: digest(vm_control.ROOT / name)
        if (vm_control.ROOT / name).is_file()
        else None
        for name in sorted(set(paths))
        if name
    }
    inputs.update(_cargo_configuration())
    return {
        "commit": vm_control.output(
            ["git", "rev-parse", "HEAD"], cwd=vm_control.ROOT
        ),
        "inputsSha256": hashlib.sha256(
            json.dumps(inputs, sort_keys=True).encode()
        ).hexdigest(),
        "inputs": inputs,
    }


def _configuration(
    component: str = "all",
) -> tuple[list[str], dict[str, str], dict[str, Any]]:
    profile = os.environ.get("DESKTOP_NATIVE_PROFILE", "release")
    if profile not in {"release", "dev"}:
        message = "DESKTOP_NATIVE_PROFILE must be release or dev"
        raise ValueError(message)
    compiler = os.environ.get("RUSTC") or shutil.which("rustc")
    if not compiler:
        message = "rustc is not available"
        raise RuntimeError(message)
    # Pin the measured compiler, including when Cargo config names another one.
    environment = {**os.environ, "CARGO_BUILD_JOBS": "1", "RUSTC": compiler}
    rustc = vm_control.output([compiler, "-vV"], cwd=vm_control.ROOT)
    host = next(
        line.removeprefix("host: ")
        for line in rustc.splitlines()
        if line.startswith("host: ")
    )
    target = os.environ.get("DESKTOP_NATIVE_TARGET", host)
    cargo = vm_control.output(["cargo", "--version"], cwd=vm_control.ROOT)
    command = [
        "cargo",
        "build",
        "--locked",
        "--profile",
        profile,
        "--jobs",
        "1",
        *[
            argument
            for role in components(component)
            for argument in (
                "--package",
                COMPONENTS[role]["package"],
                "--bin",
                COMPONENTS[role]["file"],
            )
        ],
        "--target-dir",
        str(BUILDS / "cargo-target"),
        "--target",
        target,
        "--message-format=json-render-diagnostics",
    ]
    environment_identity = {
        key: hashlib.sha256(environment[key].encode()).hexdigest()
        for key in sorted(environment)
        if key.startswith(("CARGO_", "RUST", "CC", "CXX", "AR", "LD"))
        or key in {"PATH", "LIBRARY_PATH", "CPATH", "PKG_CONFIG_PATH"}
    }
    compatibility = {
        "rustc": rustc,
        "cargo": cargo,
        "target": target,
        "profile": profile,
        "environmentValueSha256": environment_identity,
        "cargoConfiguration": _cargo_configuration(),
    }
    return command, environment, compatibility


def _cargo_artifact(messages: Path, role: str) -> dict[str, Any]:
    artifacts = []
    with messages.open(encoding="utf-8") as events:
        for line in events:
            event = json.loads(line)
            if (
                event.get("reason") == "compiler-artifact"
                and event["target"]["name"] == COMPONENTS[role]["file"]
                and "bin" in event["target"]["kind"]
                and event.get("executable")
                and not event["profile"].get("test", False)
            ):
                artifacts.append(event)
    if len(artifacts) != 1:
        message = f"Cargo did not identify exactly one {role} executable"
        raise RuntimeError(message)
    artifact = artifacts[0]
    output = Path(artifact["executable"]).resolve()
    if not output.is_relative_to((BUILDS / "cargo-target").resolve()):
        message = "Cargo executable escaped the owned compiler directory"
        raise RuntimeError(message)
    return artifact


def _build(destination: Path, lock_fd: int) -> dict[str, Any]:
    source = _source_identity()
    component = os.environ.get("NATIVE_COMPONENT", "all")
    command, environment, compatibility = _configuration(component)
    metadata = {
        "schemaVersion": SCHEMA_VERSION,
        "component": component,
        "buildId": destination.name,
        "startedAt": datetime.now(UTC).isoformat(),
        "status": "building",
        "source": source,
        "command": command,
        **compatibility,
        "cache": {
            "compilerDirectory": str(BUILDS / "cargo-target"),
            "downloadDirectory": str(
                Path(
                    environment.get("CARGO_HOME", Path.home() / ".cargo")
                ).resolve()
            ),
            "ownershipLock": str(BUILDS / "build.lock"),
            "compatibilitySha256": hashlib.sha256(
                json.dumps(compatibility, sort_keys=True).encode()
            ).hexdigest(),
        },
    }
    vm_control.save(destination / "build.json", metadata)
    messages = destination / "cargo.jsonl"
    with (
        messages.open("w", encoding="utf-8") as events,
        (destination / "build.log").open("w", encoding="utf-8") as log,
        resource_budget.claim("native", destination, 1, 1536),
    ):
        cpu = min(os.sched_getaffinity(0))
        metadata["executionCommand"] = [
            "taskset",
            "--cpu-list",
            str(cpu),
            *command,
        ]
        metadata["resourceBudget"] = {
            "cpus": 1,
            "hostCpuAffinity": [cpu],
            "memoryMiB": 1536,
            "memoryEnforcement": "admission budget, not a hard memory limit",
        }
        vm_control.save(destination / "build.json", metadata)
        started = time.monotonic()
        with subprocess.Popen(  # ruff: ignore[subprocess-without-shell-equals-true] - Fixed Cargo argv, no shell text.
            metadata["executionCommand"],
            cwd=vm_control.ROOT,
            env={
                **environment,
                resource_budget.TOKEN: os.environ[resource_budget.TOKEN],
            },
            stdout=events,
            stderr=log,
            pass_fds=(lock_fd,),
        ) as process:
            try:
                _, status, usage = os.wait4(process.pid, 0)
                process.returncode = os.waitstatus_to_exitcode(status)
            except BaseException:
                process.kill()
                raise
            metadata["resourceUsage"] = {
                "scope": "affinity-pinned Cargo process and waited descendants",
                "maxRssKiB": usage.ru_maxrss,
                "userCpuSeconds": usage.ru_utime,
                "systemCpuSeconds": usage.ru_stime,
                "elapsedSeconds": time.monotonic() - started,
            }
            metadata["cargoExitCode"] = process.returncode
            if process.returncode:
                metadata.update({
                    "status": "failed",
                    "finishedAt": datetime.now(UTC).isoformat(),
                })
            vm_control.save(destination / "build.json", metadata)
            if process.returncode:
                raise subprocess.CalledProcessError(process.returncode, command)
    return _publish(destination, metadata, compatibility)


def _publish(
    destination: Path, metadata: dict[str, Any], compatibility: dict[str, Any]
) -> dict[str, Any]:
    component = metadata["component"]
    source = metadata["source"]
    messages = destination / "cargo.jsonl"
    metadata["components"] = {}
    protocols = set()
    for role in components(component):
        artifact = _cargo_artifact(messages, role)
        output = Path(artifact["executable"]).resolve()
        binary = destination / COMPONENTS[role]["file"]
        shutil.copyfile(output, binary)
        binary.chmod(0o500)
        flag = "protocol-version" if role == "cli" else "--protocol-version"
        protocols.add(int(vm_control.output([binary, flag], timeout=10)))
        metadata["components"][role] = {
            "file": binary.name,
            "sha256": digest(binary),
            "cargoArtifact": {
                "executable": str(output),
                "packageId": artifact["package_id"],
                "profile": artifact["profile"],
                "features": artifact["features"],
                "fresh": artifact["fresh"],
            },
        }
    if len(protocols) != 1 or next(iter(protocols)) <= 0:
        message = "Selected executables report incompatible control protocols"
        raise RuntimeError(message)
    metadata["controlProtocol"] = protocols.pop()
    if "daemon" in components(component):
        shutil.copyfile(UNIT, destination / UNIT.name)
        (destination / UNIT.name).chmod(0o400)
        metadata["unitSha256"] = digest(destination / UNIT.name)
        if (
            metadata["unitSha256"]
            != source["inputs"][str(UNIT.relative_to(vm_control.ROOT))]
        ):
            message = "Service input changed during compilation"
            raise RuntimeError(message)
    if (
        source != _source_identity()
        or compatibility != _configuration(component)[2]
    ):
        message = (
            "Build inputs changed during compilation; artifact not published"
        )
        raise RuntimeError(message)
    metadata.update({
        "status": "built",
        "finishedAt": datetime.now(UTC).isoformat(),
    })
    vm_control.save(destination / "build.json", metadata)
    (destination / "build.json").chmod(0o400)
    for role in (
        *components(component),
        *(("all",) if component == "all" else ()),
    ):
        pointer = "latest.json" if role == "all" else f"latest-{role}.json"
        vm_control.save(BUILDS / pointer, {"artifact": str(destination)})
    return metadata


def main() -> None:
    """Build with one Cargo job and retain evidence for every attempt.

    Raises:
        OSError: Build inputs or output files cannot be accessed.
        ValueError: Build input metadata is malformed.
        RuntimeError: Sources change while Cargo runs.
        subprocess.SubprocessError: A compiler or identity command fails.
    """
    os.umask(0o077)
    BUILDS.mkdir(parents=True, exist_ok=True)
    with (BUILDS / "build.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        destination = BUILDS / str(uuid.uuid4())
        destination.mkdir()
        try:
            metadata = _build(destination, lock.fileno())
        except (
            OSError,
            ValueError,
            RuntimeError,
            subprocess.SubprocessError,
        ) as error:
            vm_control.save(destination / "failure.json", {"error": str(error)})
            print(
                f"Native build failed; retained evidence: {destination}",
                file=sys.stderr,
            )
            raise
        print(json.dumps({"artifact": str(destination), **metadata}, indent=2))


if __name__ == "__main__":
    main()
