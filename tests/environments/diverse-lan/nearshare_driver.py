"""Headless NearShare endpoint for the isolated diverse-LAN contract."""

import argparse
import asyncio
import json
import os
import socket
from pathlib import Path

from nearshare.core import mdns
from nearshare.core.connection import (
    Events,
    OutboundConnection,
    TransferRequest,
)
from nearshare.core.mdns import Peer
from nearshare.core.service import NearShareService

GOOGLE_ROUTE_PROBE = "172.30.45.11"


def event(name: str, **values: object) -> None:
    """Write one safe machine-readable observation."""
    print(json.dumps({"event": name, **values}, sort_keys=True), flush=True)


def comparison_salt() -> str:
    """Read the required ephemeral comparison salt without printing it.

    Returns:
        The active per-run salt.

    Raises:
        RuntimeError: The isolated runner did not supply a salt.
    """
    salt = os.environ.get("QUICKSHARE_PIN_SALT")
    if not salt:
        message = "missing comparison salt"
        raise RuntimeError(message)
    return salt


def fingerprint(value: str) -> str:
    """Return the salted PIN comparison fingerprint used by both peers."""
    result = 14695981039346656037
    for byte in f"{comparison_salt()}{value}".encode():
        result ^= byte
        result = (result * 1099511628211) & ((1 << 64) - 1)
    return format(result, "x")


def isolated_lan_address() -> list[bytes]:
    """Choose the Docker-bridge address without requiring Internet routing.

    Returns:
        The local IPv4 address encoded for the NearShare mDNS record.
    """
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.connect((GOOGLE_ROUTE_PROBE, 9))
        return [socket.inet_aton(probe.getsockname()[0])]


async def wait_peer(
    service: NearShareService, target: str, duration: float
) -> Peer:
    """Wait for the one named mDNS peer within the test budget.

    Returns:
        The discovered target peer.
    """
    async with asyncio.timeout(duration):
        while True:
            for peer in service.browser.peers.values():
                if peer.device_name == target:
                    event("discovered", peer=target)
                    return peer
            await asyncio.sleep(0.1)


async def receive(args: argparse.Namespace) -> None:
    """Advertise and accept one inbound NearShare transfer."""
    completed = asyncio.Event()
    vars(mdns)["_local_addresses"] = isolated_lan_address

    async def accept(request: TransferRequest) -> bool:
        await asyncio.sleep(0)
        event("pin", fingerprint=fingerprint(request.pin))
        event("accepted", bytes=request.total_size)
        return True

    def complete(_peer: str, paths: list[Path]) -> None:
        event("complete", files=len(paths))
        completed.set()

    events = Events(
        on_transfer_request=accept,
        on_complete=complete,
        on_error=lambda _peer, _error: event("error"),
    )
    service = NearShareService(args.name, Path(args.received), events)
    await service.start(visible=True)
    event("ready", role="receiver")
    try:
        await asyncio.wait_for(completed.wait(), args.timeout)
    finally:
        await service.stop()


async def send(args: argparse.Namespace) -> None:
    """Discover one Google-derived peer and send one file.

    Raises:
        RuntimeError: The NearShare transfer reported an error.
    """
    errors: list[str] = []
    completed = asyncio.Event()
    service = NearShareService(args.name)
    events = Events(
        on_complete=lambda _peer, _paths: completed.set(),
        on_error=lambda _peer, error: errors.append(error),
    )
    await service.browser.start()
    event("ready", role="sender")
    try:
        peer = await wait_peer(service, args.target, args.timeout)
        connection = OutboundConnection(
            peer.host,
            peer.port,
            events,
            args.name,
            [Path(args.file)],
            peer_name=peer.device_name,
        )
        task = asyncio.create_task(connection.run())
        while not task.done():
            if connection.pin:
                event("pin", fingerprint=fingerprint(connection.pin))
                break
            await asyncio.sleep(0.05)
        await task
        if errors:
            raise RuntimeError(errors[0])
        await asyncio.wait_for(completed.wait(), args.timeout)
        event("complete", files=1)
    finally:
        await service.browser.stop()


def parser() -> argparse.ArgumentParser:
    """Build the headless receiver and sender command parser.

    Returns:
        The configured role parser.
    """
    result = argparse.ArgumentParser()
    sub = result.add_subparsers(dest="role", required=True)
    for role in ("receive", "send"):
        current = sub.add_parser(role)
        current.add_argument("--name", required=True)
        current.add_argument("--timeout", type=float, required=True)
    receiver = sub.choices["receive"]
    receiver.add_argument("--received", required=True)
    sender = sub.choices["send"]
    sender.add_argument("--file", required=True)
    sender.add_argument("--target", required=True)
    return result


def main() -> None:
    """Run the selected deterministic endpoint role."""
    args = parser().parse_args()
    if args.role == "receive":
        asyncio.run(receive(args))
    else:
        asyncio.run(send(args))


if __name__ == "__main__":
    main()
