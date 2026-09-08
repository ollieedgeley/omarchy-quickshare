"""Independent Sharing peer lifecycle for one retained desktop lab."""  # ruff: ignore[implicit-namespace-package] - Standalone script beside VM helpers.

from __future__ import annotations

import fcntl
import json
import os
import shlex
import subprocess  # ruff: ignore[suspicious-subprocess-import] - Explicit host argv only.
import sys
import time
from pathlib import Path
from typing import TYPE_CHECKING, Any

import lab_lifecycle
import peer_network
import resource_budget
import vm_control

if TYPE_CHECKING:
    from collections.abc import Sequence

SOURCE = Path(__file__).resolve().parent
REFERENCE = SOURCE.parent / "nearby-linux"
OWNER_LABEL = "org.omarchy-quickshare.desktop-owner"
RUNTIME_LABEL = "org.omarchy-quickshare.peer-runtime"
FINGERPRINT_LABEL = "org.omarchy-quickshare.fingerprint"
DEVICE = "desktop-reference"
NETDEV = "desktop-reference-net"
SHA256_LENGTH = 64


def _docker(args: Sequence[str]) -> str:
    return vm_control.output([os.environ.get("DOCKER", "docker"), *args])


def _image(*, prepare: bool = False) -> dict[str, str]:
    manifest = json.loads(
        (REFERENCE / "environment.json").read_text(encoding="utf-8")
    )
    validation = vm_control.output([
        "node",
        REFERENCE / "environment.mjs",
        "validate",
    ])
    fingerprint = validation.split()[-1].rstrip(".")
    if len(fingerprint) != SHA256_LENGTH:
        message = "Reference provisioner did not report its input fingerprint"
        raise RuntimeError(message)
    if prepare:
        try:
            information = json.loads(
                _docker(["image", "inspect", manifest["image"]])
            )[0]
        except subprocess.CalledProcessError:
            information = {}
        if (
            information
            .get("Config", {})
            .get("Labels", {})
            .get(FINGERPRINT_LABEL)
            != fingerprint
        ):
            vm_control.run(["node", REFERENCE / "environment.mjs", "provision"])
    image = json.loads(_docker(["image", "inspect", manifest["image"]]))[0]
    if image["Config"].get("Labels", {}).get(FINGERPRINT_LABEL) != fingerprint:
        message = (
            "Reference image inputs differ; run desktop-peer-prepare explicitly"
        )
        raise RuntimeError(message)
    return {
        "image": image["Id"],
        "sourceFingerprint": fingerprint,
        "tag": manifest["image"],
    }


def _environment(record: dict[str, Any]) -> dict[str, str]:
    return {
        **os.environ,
        "DESKTOP_PEER_PROJECT": record["project"],
        "DESKTOP_PEER_OWNER": record["owner"],
        "DESKTOP_PEER_IMAGE": record["image"],
        "DESKTOP_PEER_RUNTIME": record["runtime"],
        "DESKTOP_PEER_NAME": record["name"],
        "DESKTOP_PEER_SUBNET": record["subnet"],
        "DESKTOP_PEER_ADDRESS": record["peerAddress"],
        "DESKTOP_RELAY_ADDRESS": record["relayAddress"],
        "DESKTOP_GUEST_ADDRESS": record["guestAddress"],
        "DESKTOP_PEER_CASES": str(Path(record["owner"]) / "peer-cases"),
        "DESKTOP_PEER_RELAY": str(SOURCE / "lan_relay.py"),
    }


def _compose(record: dict[str, Any], args: Sequence[str]) -> str:
    return vm_control.output(
        [
            os.environ.get("DOCKER", "docker"),
            "compose",
            "--file",
            SOURCE / "peer-compose.yaml",
            *args,
        ],
        env=_environment(record),
    )


def _owned(record: dict[str, Any]) -> list[dict[str, Any]]:
    identifiers = _docker([
        "ps",
        "--all",
        "--quiet",
        "--filter",
        f"label=com.docker.compose.project={record['project']}",
    ]).split()
    containers = (
        json.loads(_docker(["inspect", *identifiers])) if identifiers else []
    )
    for container in containers:
        labels = container["Config"].get("Labels", {})
        if (
            labels.get(OWNER_LABEL) != record["owner"]
            or labels.get(RUNTIME_LABEL) != record["runtime"]
            or container["Image"] != record["image"]
        ):
            message = (
                "Peer container ownership/runtime mismatch; resources retained"
            )
            raise RuntimeError(message)
    networks = _docker([
        "network",
        "ls",
        "--quiet",
        "--filter",
        f"label=com.docker.compose.project={record['project']}",
    ]).split()
    for network in (
        json.loads(_docker(["network", "inspect", *networks]))
        if networks
        else []
    ):
        labels = network.get("Labels", {})
        if (
            labels.get(OWNER_LABEL) != record["owner"]
            or labels.get(RUNTIME_LABEL) != record["runtime"]
            or not network["Internal"]
        ):
            message = (
                "Peer network ownership/runtime mismatch; resources retained"
            )
            raise RuntimeError(message)
    return containers


def _guest(owner: Path) -> dict[str, Any]:
    state = lab_lifecycle.active(owner)
    if state is None or not vm_control.running(state):
        message = (
            "Start the selected desktop lab before starting its reference peer"
        )
        raise RuntimeError(message)
    return state


def _attached(guest: dict[str, Any]) -> bool:
    peripherals = vm_control.qmp(
        guest, "qom-list", {"path": "/machine/peripheral"}
    )
    return any(item["name"] == DEVICE for item in peripherals)


def _attach(guest: dict[str, Any], record: dict[str, Any]) -> None:
    if _attached(guest):
        return
    relay = next(
        (
            container
            for container in _owned(record)
            if container["Config"]["Labels"]["com.docker.compose.service"]
            == "relay"
        ),
        None,
    )
    if relay is None:
        message = "Owned relay container is missing; resources retained"
        raise RuntimeError(message)
    networks = relay["NetworkSettings"]["Networks"]
    if (
        set(networks) != {f"{record['project']}_private"}
        or networks[f"{record['project']}_private"]["IPAddress"]
        != record["relayAddress"]
    ):
        message = "Relay is not exclusively on its owned private address"
        raise RuntimeError(message)
    # Preload CDC so cdc_subset cannot win USB autoload and use a random MAC.
    vm_control.ssh(
        guest,
        "printf '%s\\n' omarchy | sudo -S modprobe cdc_ether",
    )
    network = vm_control.qmp(
        guest, "human-monitor-command", {"command-line": "info network"}
    )
    if NETDEV not in str(network):
        vm_control.qmp(
            guest,
            "netdev_add",
            {
                "type": "stream",
                "id": NETDEV,
                "server": False,
                "addr": {
                    "type": "inet",
                    # Internal-only Docker networks do not publish ports;
                    # QEMU runs on the host and can reach their IPs directly.
                    "host": record["relayAddress"],
                    "port": "19090",
                },
                "reconnect-ms": 1000,
            },
        )
    vm_control.qmp(
        guest,
        "device_add",
        {
            "driver": "usb-net",
            "id": DEVICE,
            "netdev": NETDEV,
            "mac": record["mac"],
        },
    )


def _configure(guest: dict[str, Any], record: dict[str, Any]) -> None:
    profile, mac, address = (
        shlex.quote(record[key]) for key in ("profile", "mac", "guestAddress")
    )
    command = (
        f'set -e; if [ "$(nmcli -g GENERAL.STATE connection show {profile} '
        '2>/dev/null)" = activated ]; then exit 0; fi; '
        f"if ! nmcli connection show {profile} >/dev/null 2>&1; then "
        f"nmcli connection add type ethernet con-name {profile} "
        f"ifname '*' 802-3-ethernet.mac-address {mac} "
        f"ipv4.method manual ipv4.addresses {address}/24 "
        "ipv4.never-default yes ipv6.method disabled connection.mdns 2; fi; "
        f"nmcli --wait 10 connection up {profile}"
    )
    command = "printf '%s\\n' omarchy | sudo -S bash -c " + shlex.quote(command)
    deadline = time.monotonic() + 25
    while True:
        result = vm_control.ssh(guest, command, check=False)
        if result.returncode == 0:
            print(result.stdout.strip())
            return
        if time.monotonic() >= deadline:
            message = f"Guest private NIC is not ready: {result.stderr}"
            raise RuntimeError(message)
        time.sleep(1)


def _ready(guest: dict[str, Any], record: dict[str, Any]) -> None:
    source = (SOURCE / "peer_discovery.py").read_text(encoding="utf-8")
    command = shlex.join([
        "python3",
        "-c",
        source,
        record["guestAddress"],
        record["peerAddress"],
    ])
    evidence = json.loads(vm_control.ssh(guest, command).stdout)
    record["readiness"] = evidence
    record["containers"] = {
        container["Config"]["Labels"]["com.docker.compose.service"]: container[
            "Id"
        ]
        for container in _owned(record)
    }
    record["status"] = "ready"
    vm_control.save(Path(record["owner"]) / "peer.json", record)
    print(json.dumps(record, indent=2))


def _start(owner: Path) -> None:
    guest = _guest(owner)
    path = owner / "peer.json"
    previous = vm_control.active(path)
    record = peer_network.identity(owner, _image(), previous)
    if previous and any(
        previous.get(key) != value for key, value in record.items()
    ):
        message = (
            "Retained peer inputs differ; inspect and explicitly remove "
            "this peer first"
        )
        raise RuntimeError(message)
    containers = _owned(record)
    if not previous and containers:
        message = (
            "Peer resources exist without their ownership record; "
            "inspect before proceeding"
        )
        raise RuntimeError(message)
    if previous and previous.get("status") != "stopped":
        for container in containers:
            state = container["State"]
            if (
                not state["Running"]
                or state.get("Health", {}).get("Status") == "unhealthy"
            ):
                message = (
                    "Owned reference peer is faulty; inspect/logs then "
                    "explicitly stop/start"
                )
                raise RuntimeError(message)
    for name in ("", "outbound", "received"):
        directory = owner / "peer-cases" / name
        directory.mkdir(parents=True, exist_ok=True)
        # The image UID must traverse the mount root and write its case dirs.
        directory.chmod(0o777 if name else 0o755)
    with resource_budget.claim("peer", owner, 1, 320):
        vm_control.save(path, {**record, "status": "starting"})
        _launch(guest, record)


def _launch(guest: dict[str, Any], record: dict[str, Any]) -> None:
    path = Path(record["owner"]) / "peer.json"
    try:
        print(
            _compose(
                record,
                [
                    "up",
                    "--detach",
                    "--no-build",
                    "--no-recreate",
                    "--pull",
                    "never",
                    "--wait",
                    "--wait-timeout",
                    "45",
                ],
            )
        )
        _attach(guest, record)
        _configure(guest, record)
        _ready(guest, record)
    except (
        OSError,
        ValueError,
        RuntimeError,
        subprocess.SubprocessError,
        KeyError,
        IndexError,
        TypeError,
    ) as error:
        record["status"] = "failed"
        record["error"] = f"{type(error).__name__}: {error}"
        if isinstance(error, subprocess.CalledProcessError):
            record["stdout"] = error.stdout
            record["stderr"] = error.stderr
        vm_control.save(path, record)
        message = f"Startup failed: {record['error']}"
        raise RuntimeError(message) from error


def _retained(owner: Path) -> dict[str, Any]:
    record = vm_control.active(owner / "peer.json")
    if not record or record.get("owner") != str(owner):
        message = f"No owned reference peer record: {owner / 'peer.json'}"
        raise RuntimeError(message)
    return record


def _stop(owner: Path, *, remove: bool) -> None:
    record = _retained(owner)
    _owned(record)
    guest = lab_lifecycle.active(owner)
    if guest and vm_control.running(guest):
        if _attached(guest):
            vm_control.qmp(guest, "device_del", {"id": DEVICE})
            deadline = time.monotonic() + 10
            while _attached(guest):
                if time.monotonic() >= deadline:
                    message = "Guest NIC removal timed out; containers retained"
                    raise RuntimeError(message)
                time.sleep(0.1)
        network = vm_control.qmp(
            guest, "human-monitor-command", {"command-line": "info network"}
        )
        if NETDEV in str(network):
            vm_control.qmp(guest, "netdev_del", {"id": NETDEV})
        profile = shlex.quote(record["profile"])
        command = (
            f"if nmcli connection show {profile} >/dev/null 2>&1; "
            f"then nmcli connection delete {profile}; fi"
        )
        vm_control.ssh(
            guest,
            "printf '%s\\n' omarchy | sudo -S bash -c " + shlex.quote(command),
        )
    print(_compose(record, ["down"] if remove else ["stop"]))
    record["status"] = "removed" if remove else "stopped"
    record.pop("readiness", None)
    vm_control.save(owner / "peer.json", record)
    if remove:
        (owner / "peer.json").unlink()


def _report(owner: Path, command: str) -> None:
    record = _retained(owner)
    print(json.dumps(record, indent=2))
    containers = _owned(record)
    print(json.dumps(containers, indent=2))
    if command == "logs":
        print(_compose(record, ["logs", "--no-color", "--tail", "200"]))
    if command == "inspect":
        guest = lab_lifecycle.active(owner)
        if guest and vm_control.running(guest):
            print(
                vm_control.qmp(
                    guest,
                    "human-monitor-command",
                    {"command-line": "info network"},
                )
            )
            print(
                vm_control.ssh(
                    guest, "ip -brief address; nmcli connection show"
                ).stdout
            )


def main() -> None:
    """Run an explicitly selected lifecycle operation under the lab lock.

    Raises:
        RuntimeError: Unknown command or incompatible retained resources.
    """
    command = sys.argv[1] if len(sys.argv) > 1 else "status"
    if command == "prepare":
        print(json.dumps(_image(prepare=True), indent=2))
        return
    owner = lab_lifecycle.directory()
    owner.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (owner / "operation.lock").open("a", encoding="utf-8") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if command == "start":
            with peer_network.lock():
                _start(owner)
        elif command in {"stop", "remove"}:
            _stop(owner, remove=command == "remove")
        elif command in {"status", "inspect", "logs"}:
            _report(owner, command)
        else:
            message = f"Unknown reference peer command: {command}"
            raise RuntimeError(message)


if __name__ == "__main__":
    try:
        main()
    except (
        OSError,
        ValueError,
        RuntimeError,
        subprocess.SubprocessError,
    ) as error:
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        print(
            f"Reference peer failed: {error}. "
            "Evidence retained; use inspect/logs.",
            file=sys.stderr,
        )
        sys.exit(1)
