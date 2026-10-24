"""Orchestrate existing desktop commands without borrowing retained runtime."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import argparse
import contextlib
import fcntl
import json
import os
import signal
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Recorded bounded argv; ordinary calls use vm_control.
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import candidate
import clean_job
import environment
import resource_budget
import shared_evidence
import vm_control

ROOT = vm_control.ROOT
CACHE = vm_control.CACHE
GIB = 1024**3
KVM_API = 12


def load(path: Path) -> dict[str, Any]:
    """Return a UTF-8 JSON orchestration or artifact record."""
    return json.loads(path.read_text(encoding="utf-8"))


def save(path: Path, record: dict[str, Any]) -> None:
    """Persist state atomically before advancing execution."""
    path.parent.mkdir(parents=True, exist_ok=True)
    vm_control.save(path, record)


def initial(path: Path) -> dict[str, Any]:
    """Return unused state or a new source-linked orchestration record."""
    if path.exists():
        return load(path)
    return shared_evidence.source_identity() | {"stages": [], "recovery": []}


def verify_inputs(record: dict[str, Any]) -> None:
    """Permit immutable inputs, but reject retained runtime or external owners.

    Raises:
        RuntimeError: Source/runtime identity differs or a cache path redirects.
    """
    shared_evidence.guard(CACHE)
    if record["root"] != str(ROOT) or record["stages"]:
        message = "Preflight requires an unused orchestration record"
        raise RuntimeError(message)
    if os.environ.get("GITHUB_SHA") and (
        os.environ["GITHUB_SHA"] != record["sourceCommit"]
        or record["sourceStatus"]
    ):
        message = (
            "Shared verification requires the clean workflow source revision"
        )
        raise RuntimeError(message)
    allowed = {"bases", "native", "downloads"}
    if CACHE.exists() and any(
        p.name not in allowed or p.is_symlink() for p in CACHE.iterdir()
    ):
        message = (
            "Fresh runtime required; only base/native/download inputs allowed"
        )
        raise RuntimeError(message)
    if CACHE.parent.exists() and any(
        p != CACHE for p in CACHE.parent.iterdir()
    ):
        message = "Retained peer/source environment is not a fresh runtime"
        raise RuntimeError(message)
    overrides = (
        "TEST_ENV_CACHE",
        "DESKTOP_LAB",
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
    )
    if any(os.environ.get(name) for name in overrides):
        message = (
            "External cache/lab/Docker ownership overrides are not allowed"
        )
        raise RuntimeError(message)


def admit(facts: dict[str, Any]) -> None:
    """Require a real KVM VM, Docker cgroup v2, and measured capacity.

    Raises:
        RuntimeError: KVM or a worker resource precondition is unavailable.
    """
    with Path("/dev/kvm").open("rb+", buffering=0) as kvm:
        if fcntl.ioctl(kvm, 0xAE00, 0) != KVM_API:
            message = "Unsupported KVM API"
            raise RuntimeError(message)
        os.close(
            fcntl.ioctl(kvm, 0xAE01, 0)
        )  # KVM_CREATE_VM, not a device-exists probe.
    facts["kvmCreateVm"] = "passed"
    if any(
        facts[f"{kind}FreeBytes"] < facts[f"required{kind.title()}Bytes"]
        for kind in ("workspace", "docker")
    ):
        message = (
            "Insufficient physical storage; unrelated files remain untouched"
        )
        raise RuntimeError(message)
    if str(facts["dockerCgroupVersion"]) != "2":
        message = "Reference builder measurements require cgroup v2"
        raise RuntimeError(message)


def preflight(path: Path, *, tools: bool) -> None:
    """Persist admission success or the original refusal."""
    record = initial(path)
    save(path, record)
    try:
        verify_inputs(record)
        record["worker"] = shared_evidence.worker()
        admit(record["worker"])
        if tools:
            fingerprint, inputs, _, _ = environment.identity()
            record.update(
                baseFingerprint=fingerprint, environment=inputs, toolsReady=True
            )
    finally:
        error = sys.exception()
        record["preflight"] = "failed" if error else "passed"
        if error:
            record["preflightError"] = f"{type(error).__name__}: {error}"
        save(path, record)


def prepare_stage(
    path: Path, record: dict[str, Any], command: list[str]
) -> dict[str, Any]:
    """Record the stage and pre-launch inventory before creating its gate.

    Returns:
        The durable stage record with its deadline and exact argv.

    Raises:
        RuntimeError: The stage failed or physical headroom is insufficient.
    """
    name = command[1]
    seconds = {
        "desktop-prepare": 5400,
        "native-build": 3600,
        "desktop-candidate": 600,
        "desktop-peer-prepare": 10800,
        "desktop-clean-job": 3000,
    }[name]
    usage = shared_evidence.storage(record)
    record.setdefault("storage", []).append({"stage": name, **usage})
    save(path, record)
    if min(usage["freeBytes"].values()) < 2 * GIB:
        message = "Stage refused: less than two GiB physical headroom remains"
        raise RuntimeError(message)
    item: dict[str, Any] = {
        "name": name,
        "command": command,
        "status": "starting",
        "deadlineSeconds": seconds,
    }
    if name == "native-build":
        record["nativeBefore"] = shared_evidence.native_inventory()
    record["stages"].append(item)
    save(path, record)
    return item


def stage(path: Path, record: dict[str, Any], command: list[str]) -> None:
    """Run a gated process session only after its identity is durable.

    Raises:
        RuntimeError: The gate exits early or the stage fails.
    """
    item = prepare_stage(path, record, command)
    name = item["name"]
    log = path.parent / (name + ".log")
    log.touch(exist_ok=False)
    with (path.parent / "stage.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        read_fd, write_fd = os.pipe()
        process = None
        try:
            process = subprocess.Popen(  # ruff: ignore[subprocess-without-shell-equals-true] - Reuse the existing descriptor gate; internal argv only.
                [
                    sys.executable,
                    str(clean_job.SOURCE / "clean_job.py"),
                    "_gate",
                    str(read_fd),
                    str(lock.fileno()),
                    str(item["deadlineSeconds"]),
                    *command,
                ],
                cwd=ROOT,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                pass_fds=(read_fd, lock.fileno()),
            )
            item["worker"] = clean_job.process(process.pid)
            if item["worker"] is None:
                message = "Stage gate exited before ownership recording"
                raise RuntimeError(message)
            item["status"] = "running"
            save(path, record)
            os.write(write_fd, b"1")
            shared_evidence.capture_stream(process.stdout, log)
            process.wait()
        finally:
            os.close(read_fd)
            os.close(write_fd)
            item["status"] = "interrupted" if sys.exception() else "finished"
            if process is not None:
                try:
                    if process.poll() is None and item.get("worker"):
                        clean_job.terminate(item["worker"])
                finally:
                    process.wait(timeout=15)
            save(path, record)
        item.update(
            status="passed" if process.returncode == 0 else "failed",
            exitCode=process.returncode,
        )
        save(path, record)
    if process.returncode:
        message = f"{name} failed with exit status {process.returncode}"
        raise RuntimeError(message)


def single(directory: Path, filename: str) -> Path:
    """Select exactly one new attempt, never the newest artifact.

    Returns:
        The only newly created manifest in the selected namespace.

    Raises:
        RuntimeError: Zero or multiple manifests match this run.
    """
    paths = list(directory.glob(f"*/{filename}"))
    if len(paths) != 1:
        message = (
            f"Expected one new {filename} under {directory}, found {len(paths)}"
        )
        raise RuntimeError(message)
    return paths[0]


@contextlib.contextmanager
def ownership(
    path: Path, *, starting: bool = False
) -> Iterator[dict[str, Any]]:
    """Serialize coordinators across every state path for this checkout.

    Yields:
        The locked run/recovery record.

    Raises:
        RuntimeError: A coordinator identity is missing or still live.
    """
    lock_path = ROOT / ".cache" / "desktop-shared.lock"
    shared_evidence.guard(lock_path)
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(
        lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600
    )
    with os.fdopen(descriptor, "a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        shared_evidence.guard(CACHE)
        record = load(path)
        if starting:
            verify_inputs(record)
            coordinator = clean_job.process(os.getpid())
            if coordinator is None:
                message = "Cannot identify the run coordinator"
                raise RuntimeError(message)
            record.update(coordinator=coordinator, runFinished=False)
            save(path, record)
        elif record.get("coordinator") and not record.get("runFinished"):
            if clean_job.live(record["coordinator"]):
                message = "Run coordinator is still live; recovery refused"
                raise RuntimeError(message)
        try:
            yield record
        finally:
            if starting:
                record = load(path)
                record["runFinished"] = True
                save(path, record)


def run(path: Path) -> None:
    """Serialize existing input preparation and the installed visible journey.

    Raises:
        RuntimeError: Admission or selected input identity differs.
    """
    with ownership(path, starting=True) as record:
        if (
            record.get("preflight") != "passed"
            or not record.get("toolsReady")
            or record["stages"]
        ):
            message = "Successful unused full preflight required"
            raise RuntimeError(message)
        CACHE.parent.mkdir(parents=True, exist_ok=True)
        record["cacheOwner"] = {
            "device": CACHE.parent.stat().st_dev,
            "inode": CACHE.parent.stat().st_ino,
        }
        save(path, record)
        stage(path, record, ["make", "desktop-prepare"])
        base = CACHE / "bases" / record["baseFingerprint"] / "prepared.json"
        if load(base)["fingerprint"] != record["baseFingerprint"]:
            message = "Prepared environment identity differs from preflight"
            raise RuntimeError(message)
        stage(path, record, ["make", "native-build"])
        artifact = Path(load(CACHE / "native/latest.json")["artifact"])
        record["nativeArtifact"] = str(artifact)
        metadata = load(artifact / "build.json")
        if (
            metadata["status"] != "built"
            or metadata["source"]["commit"] != record["sourceCommit"]
        ):
            message = "Native build differs from the selected source"
            raise RuntimeError(message)
        save(path, record)
        stage(
            path,
            record,
            ["make", "desktop-candidate", f"NATIVE_ARTIFACT={artifact}"],
        )
        selected = single(CACHE / "candidates", "candidate.json").parent
        manifest = candidate.load(selected)
        if manifest["sourceHead"] != record["sourceCommit"] or manifest[
            "nativeBuild"
        ]["artifact"] != str(artifact):
            message = "Candidate source/native identity differs"
            raise RuntimeError(message)
        record["candidateDirectory"] = str(selected)
        save(path, record)
        stage(path, record, ["make", "desktop-peer-prepare"])
        stage(
            path,
            record,
            [
                "make",
                "desktop-clean-job",
                f"DESKTOP_CANDIDATE={selected}",
                f"DESKTOP_BASE={record['baseFingerprint']}",
            ],
        )
        record["job"] = str(single(clean_job.JOBS, "job.json"))
        save(path, record)


def recovery_command(command: list[str]) -> int:
    """Return the cleanup command's exact original-work-aware exit status."""
    try:
        return vm_control.run(command, cwd=ROOT).returncode
    except subprocess.CalledProcessError as error:
        return error.returncode


def recover_inputs() -> list[str]:
    """Return failures stopping the preparation guest and reference builder."""
    errors = []
    if (CACHE / "preparation.json").exists() and recovery_command([
        "make",
        "desktop-prepare-stop",
    ]):
        errors.append("Preparation guest stop failed")
    marker = resource_budget.CLAIMS / "builder.owner"
    if marker.exists():
        name = marker.read_text(encoding="utf-8").strip()
        if name != resource_budget.builder_name() or recovery_command([
            "docker",
            "buildx",
            "stop",
            name,
        ]):
            errors.append("Reference builder ownership differs or stop failed")
    return errors


def recover_journey(record: dict[str, Any]) -> list[str]:
    """Return cleanup failures without replacing the original journey result."""
    jobs = list(clean_job.JOBS.glob("*/job.json"))
    if not jobs:
        return []
    if len(jobs) != 1:
        return ["Multiple jobs in fresh namespace; recovery refused"]
    job = jobs[0]
    record["job"] = str(job)
    record["recoveryExitCode"] = recovery_command([
        "make",
        "desktop-clean-recover",
        f"DESKTOP_JOB={job}",
    ])
    result = load(job)
    record.update(
        originalResult=result["originalResult"],
        cleanupAttempts=result["cleanupAttempts"],
    )
    attempts = result["cleanupAttempts"]
    return (
        ["Clean-job cleanup did not pass"]
        if not attempts or attempts[-1]["status"] != "passed"
        else []
    )


def recover(path: Path) -> None:
    """Use owned cleanup even after interruption; retain the original outcome.

    Raises:
        RuntimeError: Ownership is unproven or cleanup remains incomplete.
    """
    with ownership(path) as record:
        if "cacheOwner" not in record:
            return
        actual = CACHE.parent.stat()
        if record["root"] != str(ROOT) or record["cacheOwner"] != {
            "device": actual.st_dev,
            "inode": actual.st_ino,
        }:
            message = "Fresh cache ownership changed; recovery refused"
            raise RuntimeError(message)
        errors = []
        for item in record["stages"]:
            if item.get("worker"):
                try:
                    clean_job.terminate(item["worker"])
                except (OSError, RuntimeError) as error:
                    errors.append(f"{item['name']}: {error}")
        errors.extend(recover_journey(record))
        errors.extend(recover_inputs())
        record["recovery"].append({
            "status": "failed" if errors else "passed",
            "errors": errors,
        })
        save(path, record)
        if errors:
            message = "; ".join(errors)
            raise RuntimeError(message)


def main() -> None:
    """Expose admission, execution, recovery and evidence staging commands."""
    os.umask(0o077)
    signal.signal(signal.SIGTERM, clean_job.interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "command", choices=("preflight", "run", "recover", "collect")
    )
    parser.add_argument("--state", type=Path, required=True)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--tools", action="store_true")
    args = parser.parse_args()
    path = args.state.resolve()
    if path.is_relative_to(ROOT):
        parser.error("--state must be outside the checkout")
    if args.command == "preflight":
        preflight(path, tools=args.tools)
    elif args.command == "run":
        run(path)
    elif args.command == "recover":
        recover(path)
    else:
        if not args.evidence:
            parser.error("collect requires --evidence")
        shared_evidence.collect(path, args.evidence.resolve())


if __name__ == "__main__":
    try:
        main()
    except (
        OSError,
        ValueError,
        RuntimeError,
        subprocess.SubprocessError,
        KeyboardInterrupt,
    ) as error:
        print(f"Shared desktop: {error}", file=sys.stderr)
        sys.exit(1)
