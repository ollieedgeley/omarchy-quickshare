"""Bound and measure the checkout-owned reference compiler."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

from __future__ import annotations

import json
import os
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Explicit owned builder argv.
import time
import uuid
from pathlib import Path
from typing import TYPE_CHECKING, Any

import resource_budget as budget

if TYPE_CHECKING:
    from collections.abc import Sequence

MIN_MEMORY_MIB = 1536
JVM_HEAP_MIB = 512
BUILDER_OVERHEAD_MIB = 512


def _memory_mib(name: str) -> int:
    override = os.environ.get("DESKTOP_REFERENCE_MEMORY_MIB")
    if override is not None:
        return int(override)
    ids = budget._docker(  # ruff: ignore[private-member-access] - Shared exact builder inspection.
        "ps", "-aq", "--filter", f"name=^/buildx_buildkit_{name}0$"
    ).split()
    if not ids:
        return 3072
    (container,) = json.loads(budget._docker("inspect", *ids))  # ruff: ignore[private-member-access] - Shared exact builder inspection.
    memory = container["HostConfig"]["Memory"]
    if (
        container["Name"] != f"/buildx_buildkit_{name}0"
        or not isinstance(memory, int)
        or memory <= 0
        or memory % (1024 * 1024)
    ):
        message = f"Retained builder has invalid memory cap: {name}"
        raise RuntimeError(message)
    return memory // (1024 * 1024)


def _limits(name: str, memory_mib: int) -> None:
    ids = budget._docker(  # ruff: ignore[private-member-access] - Internal admission/build implementation shares Docker inspection.
        "ps", "-aq", "--filter", f"name=^/buildx_buildkit_{name}0$"
    ).split()
    for container in json.loads(budget._docker("inspect", *ids)) if ids else []:  # ruff: ignore[private-member-access] - Shared internal inspection.
        config = container["HostConfig"]
        if (
            config["CpuPeriod"] != budget.BUILD_CPU_PERIOD
            or config["CpuQuota"] != budget.BUILD_CPU_PERIOD
            or config["Memory"] != memory_mib * 1024 * 1024
            or config["MemorySwap"] != config["Memory"]
        ):
            message = (
                f"Retained builder limits differ: {name}; "
                f"requested memoryMiB={memory_mib}"
            )
            raise RuntimeError(message)


def _measure(name: str) -> dict[str, Any]:
    container = json.loads(
        budget._docker("inspect", f"buildx_buildkit_{name}0")  # ruff: ignore[private-member-access] - Shared internal inspection.
    )[0]
    pid = container["State"]["Pid"]
    cgroup = budget.container_cgroup(pid, container["Id"])
    return {
        "containerId": container["Id"],
        "pid": pid,
        "cgroup": str(cgroup),
        "scope": "dedicated BuildKit cgroup lifetime since this bootstrap",
        "memoryCurrentBytes": int(
            (cgroup / "memory.current").read_text(encoding="utf-8")
        ),
        "memoryPeakBytes": int(
            (cgroup / "memory.peak").read_text(encoding="utf-8")
        ),
        "allocatedMemoryBytes": container["HostConfig"]["Memory"],
    }


def _prepare(name: str, memory_mib: int) -> None:
    docker = os.environ.get("DOCKER", "docker")
    result = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Exact checkout-owned builder.
        [docker, "buildx", "inspect", name],
        capture_output=True,
        check=False,
    )
    marker = budget.CLAIMS / "builder.owner"
    if not result.returncode and not marker.is_file():
        message = f"Builder exists without checkout ownership: {name}"
        raise RuntimeError(message)
    if result.returncode:
        subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Create scoped capped builder.
            [
                docker,
                "buildx",
                "create",
                "--name",
                name,
                "--driver",
                "docker-container",
                "--driver-opt",
                f"cpu-period=100000,cpu-quota=100000,memory={memory_mib}m,memory-swap={memory_mib}m",
            ],
            check=True,
        )
        marker.write_text(name + "\n", encoding="utf-8")
    _limits(name, memory_mib)


def _new_record(
    command: Sequence[str], name: str, memory_mib: int
) -> dict[str, Any]:
    scheduler_mib = memory_mib - JVM_HEAP_MIB - BUILDER_OVERHEAD_MIB
    return {
        "builder": name,
        "command": list(command),
        "cpus": 1,
        "memoryMiB": memory_mib,
        "jvmHeapMiB": JVM_HEAP_MIB,
        "bazelSchedulerMemoryMiB": scheduler_mib,
        "jobs": 1,
        "status": "starting",
        "startedAtUnix": time.time(),
        "elapsedScope": "builder setup, bootstrap and build before stop",
    }


def _prepare_record(record: dict[str, Any], path: Path, started: float) -> None:
    try:
        _prepare(record["builder"], record["memoryMiB"])
    except (OSError, RuntimeError, subprocess.SubprocessError) as error:
        record.update(
            status="failed",
            error=str(error),
            elapsedSeconds=time.monotonic() - started,
        )
        path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        raise


def _finish_record(record: dict[str, Any], path: Path, started: float) -> None:
    if record["status"] == "starting":
        record["status"] = "failed-or-interrupted"
    record["elapsedSeconds"] = time.monotonic() - started
    try:
        record["usage"] = _measure(record["builder"])
    except (
        OSError,
        ValueError,
        StopIteration,
        subprocess.SubprocessError,
    ) as error:
        record["measurementError"] = str(error)
    path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"referenceBuildRecord": str(path), **record}))


def _execute(record: dict[str, Any], path: Path, started: float) -> None:
    name = record["builder"]
    memory_mib = record["memoryMiB"]
    scheduler_mib = record["bazelSchedulerMemoryMiB"]
    command = record["command"]
    docker = os.environ.get("DOCKER", "docker")
    try:
        subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Bootstrap bounded owned builder.
            [docker, "buildx", "inspect", "--bootstrap", name],
            check=True,
        )
        _limits(name, memory_mib)
        result = subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Explicit build argv.
            command,
            check=False,
            env={
                **os.environ,
                "BUILDX_BUILDER": name,
                "BUILDKIT_PROGRESS": os.environ.get(
                    "BUILDKIT_PROGRESS", "plain"
                ),
                "REFERENCE_BAZEL_MEMORY_MIB": str(scheduler_mib),
                "REFERENCE_BAZEL_JVM_HEAP_MIB": str(JVM_HEAP_MIB),
            },
            pass_fds=(3,) if Path("/proc/self/fd/3").exists() else (),
        )
        record.update(
            status="built" if result.returncode == 0 else "failed",
            exitCode=result.returncode,
        )
        result.check_returncode()
    finally:
        _finish_record(record, path, started)
        subprocess.run(  # ruff: ignore[subprocess-without-shell-equals-true] - Stop only this owned builder, retain cache.
            [docker, "buildx", "stop", name],
            check=True,
        )


def run(command: Sequence[str]) -> None:
    """Run one admitted reference build and retain cgroup measurements.

    Tooling, capacity, ownership, and compilation errors propagate after
    retaining the available build evidence.

    Raises:
        ValueError: Requested builder memory is too small.
    """
    name = budget.builder_name()
    memory_mib = _memory_mib(name)
    if memory_mib < MIN_MEMORY_MIB:
        message = f"DESKTOP_REFERENCE_MEMORY_MIB must be >= {MIN_MEMORY_MIB}"
        raise ValueError(message)
    owner = budget.ROOT / "tests/environments/nearby-linux"
    with budget.claim("reference", owner, 1, memory_mib):
        records = budget.CLAIMS / "reference-builds"
        records.mkdir(exist_ok=True)
        path = records / f"{uuid.uuid4()}.json"
        started = time.monotonic()
        record = _new_record(command, name, memory_mib)
        path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
        _prepare_record(record, path, started)
        _execute(record, path, started)
