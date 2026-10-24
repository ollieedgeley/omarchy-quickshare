"""Run a disposable installed-candidate UI job, or recover its abandoned lab."""  # ruff: ignore[implicit-namespace-package] - Standalone sibling tooling.

import argparse
import fcntl
import json
import os
import shutil
import signal
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Bounded, recorded lifecycle subprocesses.
import sys
import time
import uuid
from pathlib import Path
from typing import Any

import build_native
import candidate
import clean_job_resources
import vm_control

SOURCE = Path(__file__).resolve().parent
JOBS = vm_control.CACHE / "jobs"
BOOT = Path("/proc/sys/kernel/random/boot_id")


def process(pid: int) -> dict[str, Any] | None:
    """Read Linux process incarnation and group identity without PID guessing.

    Returns:
        Current kernel identity, or None for an absent process.
    """
    try:
        fields = (
            Path(f"/proc/{pid}/stat")
            .read_text(encoding="utf-8")
            .rsplit(")", 1)[1]
            .split()
        )
    except FileNotFoundError:
        return None
    return {
        "pid": pid,
        "bootId": BOOT.read_text(encoding="utf-8").strip(),
        "startTicks": fields[19],
        "state": fields[0],
        "group": int(fields[2]),
        "session": int(fields[3]),
    }


def live(identity: dict[str, Any]) -> bool:
    """Check boot and process start identity, treating zombies as abandoned.

    Returns:
        Whether the exact recorded process can still execute.
    """
    current = process(identity["pid"])
    return bool(
        current
        and current["state"] != "Z"
        and all(
            current[key] == identity[key]
            for key in ("pid", "bootId", "startTicks")
        )
    )


def members(identity: dict[str, Any]) -> list[dict[str, Any]]:
    """Find surviving members of the recorded bounded stage session.

    Returns:
        Live processes in the stage's original boot/session.
    """
    if identity["bootId"] != BOOT.read_text(encoding="utf-8").strip():
        return []
    found = []
    for entry in Path("/proc").iterdir():
        if entry.name.isdecimal():
            current = process(int(entry.name))
            if (
                current
                and current["state"] != "Z"
                and current["session"] == identity["pid"]
            ):
                found.append(current)
    return found


def terminate(identity: dict[str, Any]) -> None:
    """Terminate only a proven abandoned stage's original process group.

    Raises:
        RuntimeError: A surviving stage cannot be proven or stopped safely.
    """
    children = members(identity)
    if not children:
        return
    current = process(identity["pid"])
    if (
        not current
        or any(
            current[key] != identity[key] for key in ("bootId", "startTicks")
        )
        or current["group"] != identity["pid"]
        or current["session"] != identity["pid"]
        or any(child["group"] != identity["pid"] for child in children)
    ):
        message = "Unproven surviving stage group; resources retained"
        raise RuntimeError(message)
    os.killpg(identity["pid"], signal.SIGTERM)
    time.sleep(1)
    # The original group/session number remains reserved while it has members.
    # Never signal a new process incarnation if the old session has vanished.
    if members(identity):
        current = process(identity["pid"])
        if current and current["startTicks"] != identity["startTicks"]:
            message = "Stage PID changed during termination; resources retained"
            raise RuntimeError(message)
        os.killpg(identity["pid"], signal.SIGKILL)
    deadline = time.monotonic() + 10
    while members(identity):
        if time.monotonic() >= deadline:
            message = "Abandoned stage did not stop; resources retained"
            raise RuntimeError(message)
        time.sleep(0.1)


def save(job: dict[str, Any]) -> None:
    """Atomically persist the job before advancing its lifecycle."""
    vm_control.save(JOBS / job["jobId"] / "job.json", job)


def stage(job: dict[str, Any], name: str, command: list[str]) -> int:
    """Gate resource creation until bounded child identity is durable.

    Returns:
        The real stage exit code, including GNU timeout's nonzero result.

    Raises:
        RuntimeError: Child identity could not be persisted before launch.
    """
    directory = JOBS / job["jobId"]
    record: dict[str, Any] = {
        "name": name,
        "command": command,
        "startedAt": candidate.now(),
        "status": "starting",
        "deadlineSeconds": job["deadlines"][name],
        "log": str(directory / f"{len(job['stages']):02d}-{name}.log"),
    }
    job["stages"].append(record)
    save(job)
    read_fd, write_fd = os.pipe()
    child = None
    try:  # ruff: ignore[too-many-statements-in-try-clause] - Gate, record and supervise one child as a single interruption boundary.
        with Path(record["log"]).open("x", encoding="utf-8") as log:
            child = subprocess.Popen(  # ruff: ignore[subprocess-without-shell-equals-true] - Explicit local argv with no shell.
                [
                    sys.executable,
                    str(SOURCE / "clean_job.py"),
                    "_gate",
                    str(read_fd),
                    str(job["lockFd"]),
                    str(record["deadlineSeconds"]),
                    *command,
                ],
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                pass_fds=(read_fd, job["lockFd"]),
                env={
                    **os.environ,
                    "DESKTOP_LAB": Path(job["ownerDirectory"]).name,
                },
            )
            identity = process(child.pid)
            if identity is None:
                message = "Stage gate exited before ownership recording"
                raise RuntimeError(message)  # ruff: ignore[raise-within-try] - The surrounding handler must reap the gated child.
            record["worker"] = identity
            record["status"] = "running"
            save(job)
            os.write(write_fd, b"1")
            code = child.wait()
            record.update({
                "exitCode": code,
                "status": "passed" if code == 0 else "failed",
            })
            return code
    except BaseException:
        if child is not None:
            identity = record.get("worker") or process(child.pid)
            if identity:
                terminate(identity)
            child.wait()
        record["status"] = "interrupted"
        raise
    finally:
        os.close(read_fd)
        os.close(write_fd)
        record["finishedAt"] = candidate.now()
        save(job)


def cleanup(job: dict[str, Any]) -> None:
    """Keep each cleanup attempt separate from the original job outcome."""
    result: dict[str, Any] = {"startedAt": candidate.now(), "status": "failed"}
    job["cleanupAttempts"].append(result)
    save(job)
    try:
        code = stage(
            job,
            "cleanup",
            [
                sys.executable,
                str(SOURCE / "clean_job_resources.py"),
                str(JOBS / job["jobId"] / "job.json"),
            ],
        )
        result.update({
            "exitCode": code,
            "status": "passed" if code == 0 else "failed",
        })
    except Exception as error:  # ruff: ignore[blind-except] - Preserve cleanup failure without replacing original failure.
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        result["finishedAt"] = candidate.now()
        result["evidence"] = str(JOBS / job["jobId"] / "cleanup-evidence")
        save(job)


def exit_code(job: dict[str, Any]) -> int:
    """Keep failed original work failed even after successful recovery.

    Returns:
        Zero only when the original journey and latest cleanup both passed.
    """
    original = job["originalResult"]
    if original["status"] != "passed":
        return original.get("exitCode", 1) or 1
    return 0 if job["cleanupAttempts"][-1]["status"] == "passed" else 1


def run(options: argparse.Namespace) -> int:
    """Create a durable job before touching its unique disposable resources.

    Returns:
        The combined original-work and cleanup exit status.

    Raises:
        RuntimeError: Required bounded process supervision is unavailable.
    """
    if shutil.which("timeout") is None:
        message = "GNU timeout is required for bounded clean-job stages"
        raise RuntimeError(message)
    identifier = str(uuid.uuid4())
    directory = JOBS / identifier
    directory.mkdir(parents=True)
    owner = (
        vm_control.CACHE / "labs" / ("job-" + identifier.replace("-", "")[:27])
    )
    with (directory / "lifetime.lock").open("x") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        job: dict[str, Any] = {
            "schemaVersion": 1,
            "kind": "disposable-clean-job",
            "jobId": identifier,
            "startedAt": candidate.now(),
            "worker": process(os.getpid()),
            "candidateDirectory": str(options.candidate),
            "baseFingerprint": options.base,
            "ownerDirectory": str(owner),
            "installationDirectory": str(
                vm_control.CACHE / "clean-installs" / identifier
            ),
            "stages": [],
            "cleanupAttempts": [],
            "lockFd": lock.fileno(),
            "originalResult": {"status": "running"},
            "deadlines": {
                "install": 1200,
                "peer-prepare": 1200,
                "peer-start": 180,
                "scenario": options.scenario_timeout,
                "cleanup": 300,
            },
        }
        save(job)
        print(f"Clean job: {directory / 'job.json'}", flush=True)
        try:  # ruff: ignore[too-many-statements-in-try-clause] - Preserve original work separately from mandatory cleanup.
            selected = options.candidate.resolve(strict=True)
            job["candidate"] = candidate.load(selected)
            job["candidateManifestSha256"] = build_native.digest(
                selected / "candidate.json"
            )
            save(job)
            installation = (
                Path(job["installationDirectory"]) / "installation.json"
            )
            commands = [
                (
                    "install",
                    [
                        sys.executable,
                        str(SOURCE / "clean_install.py"),
                        "--candidate",
                        str(selected),
                        "--lab",
                        owner.name,
                        "--base",
                        options.base,
                        "--attempt",
                        str(installation.parent),
                    ],
                ),
                (
                    "peer-prepare",
                    [
                        sys.executable,
                        str(SOURCE / "reference_peer.py"),
                        "prepare",
                    ],
                ),
                (
                    "peer-start",
                    [
                        sys.executable,
                        str(SOURCE / "reference_peer.py"),
                        "start",
                    ],
                ),
                (
                    "scenario",
                    [
                        sys.executable,
                        str(SOURCE / "ui_scenario.py"),
                        "--installation",
                        str(installation),
                    ],
                ),
            ]
            for name, command in commands:
                job["originalResult"] = {"status": "running", "stage": name}
                save(job)
                code = stage(job, name, command)
                if code:
                    job["originalResult"] = {
                        "status": "failed",
                        "stage": name,
                        "exitCode": code if code >= 0 else 128 - code,
                    }
                    break
            else:
                job["originalResult"] = {
                    "status": "passed",
                    "stage": "scenario",
                    "exitCode": 0,
                }
        except (Exception, KeyboardInterrupt) as error:  # ruff: ignore[blind-except] - Persist original failure before the shared cleanup path.
            job["originalResult"] = {
                "status": "interrupted"
                if isinstance(error, KeyboardInterrupt)
                else "failed",
                "exitCode": 130 if isinstance(error, KeyboardInterrupt) else 1,
                "error": f"{type(error).__name__}: {error}",
            }
        finally:
            job["originalResult"]["finishedAt"] = candidate.now()
            save(job)
            cleanup(job)
        return exit_code(job)


def recover(path: Path) -> int:
    """Prove abandonment, stop its stage, then reuse ordinary owned cleanup.

    Returns:
        The original result combined with the latest cleanup result.

    Raises:
        RuntimeError: The path, live worker, child or lock cannot be reclaimed.
    """
    path = path.resolve(strict=True)
    job = json.loads(path.read_text(encoding="utf-8"))
    clean_job_resources.owner_for(job)
    if path != JOBS / job["jobId"] / "job.json":
        message = "Recovery requires the original durable job.json path"
        raise RuntimeError(message)
    with (path.parent / "recovery.lock").open("a") as recovery_lock:
        fcntl.flock(recovery_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        job = json.loads(path.read_text(encoding="utf-8"))
        if live(job["worker"]):
            message = "Clean-job worker is still live; recovery refused"
            raise RuntimeError(message)
        for record in job["stages"]:
            if "worker" in record:
                terminate(record["worker"])
        with (path.parent / "lifetime.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            job["lockFd"] = lock.fileno()
            job["recoveryWorker"] = process(os.getpid())
            if job["originalResult"]["status"] == "running":
                job["originalResult"] = {
                    "status": "interrupted",
                    "exitCode": 130,
                    "error": "Original worker died before recording a result",
                    "finishedAt": candidate.now(),
                }
            save(job)
            cleanup(job)
            return exit_code(job)


def interrupted(_signum: int, _frame: object) -> None:
    """Turn ordinary termination into the same finally-cleanup path.

    Raises:
        KeyboardInterrupt: The job received an operator termination signal.
    """
    raise KeyboardInterrupt


def main() -> int:
    """Parse explicit run/recover commands and the private gated stage launch.

    Returns:
        A truthful original-versus-cleanup exit status.
    """
    os.umask(0o077)
    if len(sys.argv) > 1 and sys.argv[1] == "_gate":
        gate = int(sys.argv[2])
        if os.read(gate, 1) != b"1":
            return 1
        os.close(gate)
        os.set_inheritable(int(sys.argv[3]), True)  # ruff: ignore[boolean-positional-value-in-call] - Python's POSIX binding requires positional-only arguments.
        os.execvp(  # ruff: ignore[start-process-with-no-shell] - Replace the gated child with the required bounded supervisor.
            "timeout",  # ruff: ignore[start-process-with-partial-path] - Required host GNU timeout, checked before launch.
            [
                "timeout",
                "--signal=INT",
                "--kill-after=10s",
                sys.argv[4],
                *sys.argv[5:],
            ],
        )
    signal.signal(signal.SIGTERM, interrupted)
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--candidate", required=True, type=Path)
    run_parser.add_argument("--base", required=True)
    run_parser.add_argument("--scenario-timeout", type=int, default=120)
    recover_parser = commands.add_parser("recover")
    recover_parser.add_argument("--job", required=True, type=Path)
    options = parser.parse_args()
    if options.command == "recover":
        return recover(options.job)
    if not options.candidate.is_absolute() or options.scenario_timeout < 1:
        parser.error(
            "--candidate must be absolute; --scenario-timeout must be positive"
        )
    return run(options)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (Exception, KeyboardInterrupt) as error:  # ruff: ignore[blind-except] - CLI boundary leaves durable evidence and reports refusal/failure.
        print(
            f"Clean job failed: {type(error).__name__}: {error}",
            file=sys.stderr,
        )
        sys.exit(1)
