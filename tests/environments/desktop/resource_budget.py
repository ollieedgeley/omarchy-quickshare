"""Joint owned-host admission. CLI: report; run KIND OWNER CPU MIB -- COMMAND.

Use claim(kind, owner, cpus, memory_mib) around scenario work. Child processes
must inherit the environment. The short admission flock is never held while
work runs. Claims survive their parent through inherited, unique process tokens.
"""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

from __future__ import annotations

import argparse
import contextlib
import fcntl
import hashlib
import json
import math
import os
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Explicit host-tool argv.
import sys
import uuid
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterator
    from typing import Any


ROOT = Path(__file__).resolve().parents[3]
CACHE = ROOT / ".cache/test-env/desktop"
CLAIMS = CACHE / "capacity"
MIN_HEADROOM_MIB = 512
BUILD_CPU_PERIOD = 100000
TOKEN: str = "QUICKSHARE_CAPACITY_CLAIMS"  # ruff: ignore[hardcoded-password-string]  # non-secret env key name
BOOT = (
    Path("/proc/sys/kernel/random/boot_id").read_text(encoding="utf-8").strip()
)


def _process(pid: int) -> str | None:
    try:
        fields = (
            Path(f"/proc/{pid}/stat")
            .read_text(encoding="utf-8")
            .rsplit(")", 1)[1]
            .split()
        )
        return fields[19] if fields[0] != "Z" else None
    except (FileNotFoundError, ProcessLookupError):
        return None


def _tokens() -> set[str]:
    tokens: set[str] = set()
    for process in Path("/proc").iterdir():
        if not process.name.isdigit():
            continue
        try:
            if (
                process.stat().st_uid != os.getuid()
                or _process(int(process.name)) is None
            ):
                continue
            for entry in (process / "environ").read_bytes().split(b"\0"):
                if entry.startswith((TOKEN + "=").encode()):
                    tokens.update(entry.split(b"=", 1)[1].decode().split(","))
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            continue
    return tokens


def _docker(*args: str) -> str:
    return subprocess.check_output(  # ruff: ignore[subprocess-without-shell-equals-true] - Explicit Docker argv.
        [os.environ.get("DOCKER", "docker"), *args],
        text=True,
        timeout=3,
    ).strip()


def builder_name() -> str:
    """Return this checkout's dedicated, bounded BuildKit owner."""
    return "quickshare-" + hashlib.sha256(str(ROOT).encode()).hexdigest()[:16]


def container_cgroup(pid: int, container_id: str) -> Path:
    """Resolve the exact Docker cgroup, including nested executor groups.

    Returns:
        The container-ID cgroup ancestor, never a broader host cgroup.

    Raises:
        ValueError: The process has no identifiable owned Docker ancestor.
    """
    group = next(
        line[3:]
        for line in Path(f"/proc/{pid}/cgroup")
        .read_text(encoding="utf-8")
        .splitlines()
        if line.startswith("0::")
    )
    path = Path("/sys/fs/cgroup") / group.lstrip("/")
    for ancestor in (path, *path.parents):
        if ancestor.name in {container_id, f"docker-{container_id}.scope"}:
            return ancestor
    message = (
        f"No exact Docker cgroup ancestor for container {container_id}, "
        f"PID {pid}"
    )
    raise ValueError(message)


def _resident(pid: int) -> int:
    try:
        fields = Path(f"/proc/{pid}/statm").read_text(encoding="utf-8").split()
        return int(fields[1]) * os.sysconf("SC_PAGE_SIZE") // (1024 * 1024)
    except (
        FileNotFoundError,
        ProcessLookupError,
        PermissionError,
        StopIteration,
        ValueError,
    ):
        # No observed residency means the entire allocation stays reserved.
        return 0


def _container_memory(pid: int, container_id: str) -> dict[str, int | None]:
    try:
        group = container_cgroup(pid, container_id)
        current = int((group / "memory.current").read_text(encoding="utf-8"))
        stats = dict(
            line.split()
            for line in (group / "memory.stat")
            .read_text(encoding="utf-8")
            .splitlines()
        )
        # MemAvailable already credits reclaimable file/slab cache.
        # Only non-reclaimable residency reduces the remaining reservation.
        credit = max(
            0,
            int(stats["anon"])
            + int(stats["shmem"])
            + int(stats["kernel"])
            - int(stats["slab_reclaimable"]),
        )
        return {
            "residentMiB": min(current, credit) // (1024 * 1024),
            "cgroupCurrentBytes": current,
        }
    except (OSError, ValueError, KeyError, StopIteration):
        return {"residentMiB": 0, "cgroupCurrentBytes": None}


def _containers() -> list[dict[str, Any]]:  # ruff: ignore[too-many-branches] - Three explicit owned container classes.
    records: list[dict[str, Any]] = []
    # Inspect only this project's labelled peers and its dedicated builder.
    ids = set(
        _docker(
            "ps", "-q", "--filter", "label=org.omarchy-quickshare.desktop-owner"
        ).split()
    )
    ids.update(
        _docker(
            "ps", "-q", "--filter", f"name=^/buildx_buildkit_{builder_name()}0$"
        ).split()
    )
    ids.update(
        _docker(
            "ps",
            "-q",
            "--filter",
            "label=com.docker.compose.project=omarchy-quickshare-nearby-linux",
        ).split()
    )
    for item in json.loads(_docker("inspect", *sorted(ids))) if ids else []:
        labels = item["Config"].get("Labels", {})
        owner = labels.get("org.omarchy-quickshare.desktop-owner")
        kind = "peer"
        if owner:
            if not Path(owner).resolve().is_relative_to(CACHE / "labs"):
                continue
        elif item["Name"] == f"/buildx_buildkit_{builder_name()}0":
            kind, owner = (
                "reference",
                str(ROOT / "tests/environments/nearby-linux"),
            )
        else:
            working = labels.get("com.docker.compose.project.working_dir", "")
            if (
                Path(working).resolve()
                != ROOT / "tests/environments/nearby-linux"
            ):
                continue
            owner = str(ROOT / "tests/environments/nearby-linux")
        config = item["HostConfig"]
        cpu = config.get("NanoCpus", 0) / 1_000_000_000
        if not cpu and config.get("CpuPeriod", 0):
            cpu = config.get("CpuQuota", 0) / config["CpuPeriod"]
        memory = config.get("Memory", 0) // (1024 * 1024)
        if cpu <= 0 or memory <= 0:
            msg = (
                f"Unbounded owned container {item['Name']}: "
                f"CPU={cpu}, memoryMiB={memory}"
            )
            raise RuntimeError(msg)
        records.append({
            "kind": kind,
            "owner": owner,
            "cpus": cpu,
            "memoryMiB": memory,
            "resource": item["Id"],
            **_container_memory(item["State"]["Pid"], item["Id"]),
        })
    return records


def _affinity(pid: int) -> list[int] | None:
    """Return the union only after inspecting a stable, complete thread set."""
    tasks = Path(f"/proc/{pid}/task")
    try:
        identity = _process(pid)
        before = {int(task.name) for task in tasks.iterdir()}
        masks = [os.sched_getaffinity(tid) for tid in before]
        after = {int(task.name) for task in tasks.iterdir()}
        if (
            identity is None
            or identity != _process(pid)
            or before != after
            or not masks
            or any(not mask for mask in masks)
        ):
            return None
    except OSError:
        # Disappearing or inaccessible threads cannot justify a lower charge.
        return None
    return sorted(set().union(*masks))


def _guests() -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    seen: set[int] = set()
    paths = [
        CACHE / "active.json",
        CACHE / "preparation.json",
        *CACHE.glob("labs/*/state.json"),
    ]
    for path in paths:
        if not path.is_file():
            continue
        state = json.loads(path.read_text(encoding="utf-8"))
        directory = Path(state.get("directory", "")).resolve()
        if not directory.is_relative_to(CACHE) or not state.get("uuid"):
            continue
        try:
            pid = state.get("pid") or int(
                (directory / "qemu.pid").read_text(encoding="utf-8")
            )
            args = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
        except (FileNotFoundError, ProcessLookupError):
            continue
        if (
            b"-uuid" not in args
            or args[args.index(b"-uuid") + 1].decode() != state["uuid"]
        ):
            continue
        if pid in seen:
            continue
        seen.add(pid)
        # Read QEMU's actual allocations, not stale requested state.
        cpus = int(args[args.index(b"-smp") + 1].split(b",")[0])
        memory = int(args[args.index(b"-m") + 1])
        affinity = _affinity(pid)
        records.append({
            "kind": "vm",
            "owner": str(directory),
            "cpus": min(cpus, len(affinity)) if affinity is not None else cpus,
            "guestVcpus": cpus,
            "hostCpuAffinity": affinity,
            "memoryMiB": memory,
            "resource": pid,
            "residentMiB": _resident(pid),
        })
    return records


def _snapshot() -> dict[str, Any]:  # ruff: ignore[too-many-branches] - Reconcile live and pending ownership under one short lock.
    memory = dict(
        line.split(":", 1)
        for line in Path("/proc/meminfo")
        .read_text(encoding="utf-8")
        .splitlines()
    )
    live = _guests() + _containers()
    tokens = _tokens()
    claims: list[dict[str, Any]] = []
    for path in CLAIMS.glob("*.json"):
        record = json.loads(path.read_text(encoding="utf-8"))
        alive = record["boot"] == BOOT and (
            (
                record["start"] is not None
                and _process(record["pid"]) == record["start"]
            )
            or record["token"] in tokens
        )
        if alive:
            claims.append(record)
        else:
            path.unlink()
    # During a launch, count the reservation or observed resource, not both.
    totals: dict[tuple[str, str], tuple[float, int]] = {}
    for record in live:
        key = record["kind"], record["owner"]
        cpu, ram = totals.get(key, (0, 0))
        totals[key] = cpu + record["cpus"], ram + record["memoryMiB"]
    pending_memory = sum(
        max(0, item["memoryMiB"] - item["residentMiB"]) for item in live
    )
    for record in claims:
        key = record["kind"], record["owner"]
        cpu, ram = totals.get(key, (0, 0))
        if record["kind"] in {"vm", "peer", "reference"}:
            pending_memory += max(0, record["memoryMiB"] - ram)
            totals[key] = (
                cpu
                if record["kind"] == "vm" and cpu
                else max(cpu, record["cpus"]),
                max(ram, record["memoryMiB"]),
            )
        else:
            pending_memory += record["memoryMiB"]
            totals[key] = cpu + record["cpus"], ram + record["memoryMiB"]
    headroom = float(os.environ.get("DESKTOP_HOST_HEADROOM_MIB", "1024"))
    cpu_headroom = float(os.environ.get("DESKTOP_HOST_HEADROOM_CPUS", "1"))
    if (
        not math.isfinite(headroom)
        or headroom < MIN_HEADROOM_MIB
        or not math.isfinite(cpu_headroom)
        or cpu_headroom < 1
    ):
        msg = "Interactive headroom requires >=512 MiB and >=1 CPU"
        raise RuntimeError(msg)
    return {
        "hostCpus": len(os.sched_getaffinity(0)),
        "hostMemoryMiB": int(memory["MemTotal"].split()[0]) // 1024,
        "availableMiB": int(memory["MemAvailable"].split()[0]) // 1024,
        "headroomMiB": headroom,
        "headroomCpus": cpu_headroom,
        "allocatedCpus": sum(v[0] for v in totals.values()),
        "allocatedMiB": sum(v[1] for v in totals.values()),
        "pendingMiB": pending_memory,
        "resources": live,
        "claims": claims,
    }


@contextlib.contextmanager
def _lock() -> Iterator[None]:
    CLAIMS.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (CLAIMS / "admission.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        yield


def report() -> dict[str, Any]:
    """Return measured host headroom, owned allocations and live reservations.

    Returns:
        dict with hostCpus, hostMemoryMiB, availableMiB, headroom*, allocated*,
        pendingMiB, resources, claims.
    """
    with _lock():
        return _snapshot()


@contextlib.contextmanager
def claim(  # ruff: ignore[too-many-arguments, too-many-positional-arguments, too-many-branches] - Owner and two independent allocation dimensions.
    kind: str, owner: str | Path, cpus: float, memory_mib: int
) -> Iterator[dict[str, Any]]:
    """Reserve bounded work; inherited tokens keep orphaned children charged.

    Yields:
        The reservation record dict yielded to caller.

    Raises:
        ValueError: if cpus or memory_mib not positive.
        RuntimeError: if admission capacity exceeded.
    """
    if not math.isfinite(cpus) or cpus <= 0 or memory_mib <= 0:
        msg = "CPU and memory reservations must be positive"
        raise ValueError(msg)
    token = uuid.uuid4().hex
    record: dict[str, Any] = {
        "kind": kind,
        "owner": str(Path(owner).resolve()),
        "cpus": cpus,
        "memoryMiB": memory_mib,
        "token": token,
        "pid": os.getpid(),
        "start": _process(os.getpid()),
        "boot": BOOT,
    }
    path = CLAIMS / f"{token}.json"
    with _lock():
        snapshot = _snapshot()
        owned = [
            item
            for item in snapshot["resources"]
            if item["owner"] == record["owner"] and item["kind"] == kind
        ]
        if kind in {"vm", "peer"}:
            cpus = max(0, cpus - sum(item["cpus"] for item in owned))
            memory_mib = max(
                0, memory_mib - sum(item["memoryMiB"] for item in owned)
            )
        conflict = kind in {"vm", "peer", "reference"} and any(
            item["owner"] == record["owner"] and item["kind"] == kind
            for item in snapshot["claims"]
        )
        conflict = conflict or (kind == "reference" and bool(owned))
        if (
            conflict
            or snapshot["allocatedCpus"] + cpus + snapshot["headroomCpus"]
            > snapshot["hostCpus"]
            or snapshot["allocatedMiB"] + memory_mib + snapshot["headroomMiB"]
            > snapshot["hostMemoryMiB"]
            or snapshot["pendingMiB"] + memory_mib + snapshot["headroomMiB"]
            > snapshot["availableMiB"]
        ):
            msg = (
                "Capacity admission rejected; swap is not headroom: "
                + json.dumps(
                    {"request": record, "capacity": snapshot}, sort_keys=True
                )
            )
            raise RuntimeError(msg)
        path.write_text(json.dumps(record) + "\n", encoding="utf-8")
    previous = os.environ.get(TOKEN)
    os.environ[TOKEN] = ",".join(filter(None, (previous, token)))
    try:
        yield record
    finally:
        if previous is None:
            os.environ.pop(TOKEN, None)
        else:
            os.environ[TOKEN] = previous
        with _lock():
            # A crashed launcher or surviving descendant must stay accounted.
            if token not in _tokens():
                path.unlink(missing_ok=True)
            else:
                record["pid"], record["start"] = 0, None
                path.write_text(json.dumps(record) + "\n", encoding="utf-8")


def scenario_claim(
    owner: str | Path, *, cpus: float = 1, memory_mib: int = 128
) -> contextlib.AbstractContextManager[dict[str, Any]]:
    """Reserve scenario orchestration, in addition to its VM and peer.

    Returns:
        Context manager yielding the retained claim record.
    """
    return claim("scenario", owner, cpus, memory_mib)


def main() -> None:
    """Report capacity or execute one admitted command."""
    if sys.argv[1:2] == ["reference-build"]:
        import reference_build  # ruff: ignore[import-outside-top-level] - CLI-only helper imports the admission module.

        reference_build.run(sys.argv[2:])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["report", "run", "reference-build"])
    parser.add_argument("kind", nargs="?")
    parser.add_argument("owner", nargs="?")
    parser.add_argument("cpus", nargs="?", type=float)
    parser.add_argument("memory_mib", nargs="?", type=int)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.mode == "report":
        print(json.dumps(report(), indent=2))
        return
    command = args.command
    if command[:1] == ["--"]:
        command = command[1:]
    if not command or args.cpus is None or args.memory_mib is None:
        parser.error("run requires KIND OWNER CPU MIB -- COMMAND")
    with claim(args.kind, args.owner, args.cpus, args.memory_mib):
        subprocess.run(command, check=True)  # ruff: ignore[subprocess-without-shell-equals-true]


if __name__ == "__main__":
    main()
