"""Allocate nonoverlapping private networks for retained desktop peers."""  # ruff: ignore[implicit-namespace-package] - Standalone desktop tooling.

from __future__ import annotations

import contextlib
import fcntl
import hashlib
import ipaddress
import json
import os
from pathlib import Path
from typing import TYPE_CHECKING, Any

import vm_control

if TYPE_CHECKING:
    from collections.abc import Iterator

SOURCE = Path(__file__).resolve().parent
POOL = ipaddress.IPv4Network("10.203.0.0/16")
SUBNET_PREFIX = 24


@contextlib.contextmanager
def lock() -> Iterator[None]:
    """Serialize subnet selection through Compose creation across named labs."""
    vm_control.CACHE.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (vm_control.CACHE / "peer-network.lock").open("a") as allocation:
        fcntl.flock(allocation, fcntl.LOCK_EX)
        yield


def _occupied(
    project: str, previous: dict[str, Any] | None
) -> list[ipaddress.IPv4Network]:
    docker = os.environ.get("DOCKER", "docker")
    identifiers = vm_control.output([docker, "network", "ls", "-q"]).split()
    networks = (
        json.loads(
            vm_control.output([docker, "network", "inspect", *identifiers])
        )
        if identifiers
        else []
    )
    occupied = []
    for network in networks:
        subnets = [
            ipaddress.ip_network(config["Subnet"])
            for config in network["IPAM"].get("Config") or []
            if config.get("Subnet")
        ]
        if network["Name"] == f"{project}_private":
            labels = network.get("Labels") or {}
            if (
                previous is None
                or labels.get("org.omarchy-quickshare.desktop-owner")
                != previous["owner"]
                or labels.get("org.omarchy-quickshare.peer-runtime")
                != previous["runtime"]
                or not network["Internal"]
                or subnets != [ipaddress.ip_network(previous["subnet"])]
            ):
                message = (
                    "Existing peer network differs from retained ownership"
                )
                raise RuntimeError(message)
            continue
        occupied.extend(
            subnet
            for subnet in subnets
            if isinstance(subnet, ipaddress.IPv4Network)
        )
    return occupied


def _subnet(
    project: str, preferred: int, previous: dict[str, Any] | None
) -> ipaddress.IPv4Network:
    occupied = _occupied(project, previous)
    if previous:
        subnet = ipaddress.IPv4Network(previous["subnet"])
        if subnet.prefixlen != SUBNET_PREFIX or not subnet.subnet_of(POOL):
            message = "Retained peer subnet is outside the private /24 pool"
            raise RuntimeError(message)
        if any(subnet.overlaps(other) for other in occupied):
            message = "Retained peer subnet overlaps another Docker network"
            raise RuntimeError(message)
        return subnet
    for offset in range(256):
        subnet = ipaddress.IPv4Network(
            f"10.203.{(preferred + offset) % 256}.0/{SUBNET_PREFIX}"
        )
        if not any(subnet.overlaps(other) for other in occupied):
            return subnet
    message = "No free peer /24 remains in 10.203.0.0/16; networks retained"
    raise RuntimeError(message)


def identity(
    owner: Path, image: dict[str, str], previous: dict[str, Any] | None
) -> dict[str, Any]:
    """Select an available subnet, or preserve the retained owner's addresses.

    Hold lock() through Docker creation. Ownership conflicts or pool exhaustion
    fail without changing existing networks.

    Returns:
        Runtime identity including stable, nonoverlapping private addresses.
    """
    owner_hash = hashlib.sha256(str(owner).encode()).hexdigest()
    runtime = hashlib.sha256(json.dumps(image, sort_keys=True).encode())
    for name in (
        "peer-compose.yaml",
        "lan_relay.py",
        "peer_discovery.py",
        "peer_network.py",
        "reference_peer.py",
    ):
        runtime.update((SOURCE / name).read_bytes())
    project = f"qs-peer-{owner_hash[:16]}"
    subnet = _subnet(project, int(owner_hash[:2], 16), previous)
    return {
        **image,
        "owner": str(owner),
        "project": project,
        "runtime": runtime.hexdigest(),
        "name": f"reference-{owner.name}",
        "subnet": str(subnet),
        "peerAddress": str(subnet[10]),
        "relayAddress": str(subnet[11]),
        "guestAddress": str(subnet[250]),
        "mac": "52:54:"
        + ":".join(owner_hash[index : index + 2] for index in range(0, 8, 2)),
        "profile": f"quickshare-peer-{owner_hash[:12]}",
    }
