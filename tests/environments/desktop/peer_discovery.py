"""Probe real Sharing DNS-SD and its advertised listener from the lab guest."""  # ruff: ignore[implicit-namespace-package] - Sent to the guest as a standalone script.

import json
import socket
import struct
import sys
import time
from dataclasses import dataclass, field

SERVICE = "_FC9F5ED42C8A._tcp.local."
MDNS = ("224.0.0.251", 5353)
POINTER = 0xC0
MAX_LABEL = 63
RECORD_PTR = 12
RECORD_TXT = 16
RECORD_SRV = 33
SRV_HEADER = 6
IPV4_SIZE = 4


def _name(packet: bytes, offset: int) -> tuple[str, int]:
    labels = []
    end = None
    seen = set()
    while True:
        if offset in seen:
            message = "Cyclic DNS name"
            raise ValueError(message)
        seen.add(offset)
        length = packet[offset]
        if length & POINTER == POINTER:
            if end is None:
                end = offset + 2
            offset = ((length & MAX_LABEL) << 8) | packet[offset + 1]
            continue
        offset += 1
        if length == 0:
            return ".".join(labels) + ".", end or offset
        if length > MAX_LABEL or offset + length > len(packet):
            message = "Invalid DNS label"
            raise ValueError(message)
        labels.append(packet[offset : offset + length].decode("utf-8"))
        offset += length


def _records(packet: bytes) -> list[tuple[str, int, int, int]]:
    _, flags, questions, answers, authorities, additional = struct.unpack_from(
        "!6H", packet
    )
    if not flags & 0x8000:
        return []
    offset = 12
    for _ in range(questions):
        _, offset = _name(packet, offset)
        offset += 4
    records = []
    for _ in range(answers + authorities + additional):
        name, offset = _name(packet, offset)
        kind, _, ttl, size = struct.unpack_from("!HHIH", packet, offset)
        offset += 10
        if offset + size > len(packet):
            message = "Truncated DNS record"
            raise ValueError(message)
        if ttl:
            records.append((name, kind, offset, size))
        offset += size
    return records


def _query(connection: socket.socket, name: str, kind: int) -> None:
    encoded = (
        b"".join(
            bytes([len(label)]) + label.encode()
            for label in name.rstrip(".").split(".")
        )
        + b"\0"
    )
    packet = struct.pack("!6H", 0, 0, 1, 0, 0, 0) + encoded
    connection.sendto(packet + struct.pack("!HH", kind, 1), MDNS)


@dataclass
class _Discovery:
    instances: set[str] = field(default_factory=set)
    endpoints: dict[str, tuple[str, int]] = field(default_factory=dict)
    addresses: dict[str, str] = field(default_factory=dict)
    texts: dict[str, str] = field(default_factory=dict)

    def update(self, packet: bytes) -> None:
        for name, kind, offset, size in _records(packet):
            if kind == RECORD_PTR and name.lower() == SERVICE.lower():
                self.instances.add(_name(packet, offset)[0])
            elif kind == RECORD_SRV and size >= SRV_HEADER:
                port = struct.unpack_from("!H", packet, offset + 4)[0]
                self.endpoints[name] = (
                    _name(packet, offset + SRV_HEADER)[0],
                    port,
                )
            elif kind == 1 and size == IPV4_SIZE:
                self.addresses[name] = socket.inet_ntoa(
                    packet[offset : offset + size]
                )
            elif kind == RECORD_TXT and size:
                self.texts[name] = packet[offset : offset + size].hex()

    def query(self, connection: socket.socket) -> None:
        _query(connection, SERVICE, RECORD_PTR)
        for instance in self.instances:
            _query(connection, instance, RECORD_SRV)
            _query(connection, instance, RECORD_TXT)
        for host, _ in self.endpoints.values():
            _query(connection, host, 1)

    def reachable(self, peer: str) -> dict[str, object] | None:
        for instance in (
            self.instances & self.endpoints.keys() & self.texts.keys()
        ):
            host, port = self.endpoints[instance]
            if self.addresses.get(host) == peer:
                with socket.create_connection((peer, port), timeout=2):
                    return {
                        "service": SERVICE,
                        "instance": instance,
                        "address": peer,
                        "port": port,
                        "txtHex": self.texts[instance],
                    }
        return None


def discover(guest: str, peer: str) -> dict[str, object]:
    """Resolve the external Sharing profile and connect to its private listener.

    Returns:
        The observed service instance, TXT bytes and reachable TCP endpoint.

    Raises:
        TimeoutError: No complete, reachable Sharing advertisement arrived.
    """
    discovery = _Discovery()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as connection:
        connection.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        connection.bind(("", MDNS[1]))
        connection.setsockopt(
            socket.IPPROTO_IP, socket.IP_MULTICAST_IF, socket.inet_aton(guest)
        )
        connection.setsockopt(socket.IPPROTO_IP, socket.IP_MULTICAST_TTL, 255)
        connection.setsockopt(
            socket.IPPROTO_IP,
            socket.IP_ADD_MEMBERSHIP,
            socket.inet_aton(MDNS[0]) + socket.inet_aton(guest),
        )
        connection.settimeout(1)
        deadline = time.monotonic() + 20
        next_query = 0.0
        while time.monotonic() < deadline:
            if time.monotonic() >= next_query:
                discovery.query(connection)
                next_query = time.monotonic() + 1
            try:
                packet, _ = connection.recvfrom(65535)
            except TimeoutError:
                continue
            try:
                discovery.update(packet)
            except (IndexError, UnicodeError, ValueError, struct.error):
                continue
            evidence = discovery.reachable(peer)
            if evidence:
                return evidence
    message = f"No reachable {SERVICE} on private peer {peer}"
    raise TimeoutError(message)


if __name__ == "__main__":
    print(json.dumps(discover(sys.argv[1], sys.argv[2]), sort_keys=True))
