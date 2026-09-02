"""Deterministic peer helpers for the Android Nearby admission probe."""

import base64
import hashlib
import json
import time
from collections.abc import Callable
from dataclasses import dataclass

POLL_SECONDS = 0.05
TIMEOUT_SECONDS = 15.0


def sha256(payload: bytes) -> str:
    """Return the payload digest reported by the Android snippet.

    Returns:
        The lowercase hexadecimal SHA-256 digest.
    """
    return hashlib.sha256(payload).hexdigest()


def encode(payload: bytes) -> str:
    """Encode payload bytes for the snippet RPC boundary.

    Returns:
        The base64 text accepted by the snippet.
    """
    return base64.b64encode(payload).decode("ascii")


def wait_for(description: str, read: Callable[[], str | None]) -> str:
    """Poll a peer observation until it exists or the case budget expires.

    Returns:
        The first available observation.

    Raises:
        TimeoutError: If the observation does not arrive within the budget.
    """
    deadline = time.monotonic() + TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        result = read()
        if result is not None:
            return result
        time.sleep(POLL_SECONDS)
    message = f"Timed out waiting for {description}"
    raise TimeoutError(message)


class Peer:
    """Observable operations for one Android Nearby probe peer."""

    def __init__(self, device: object, name: str) -> None:
        """Wrap a Mobly Android device with its stable scenario name."""
        self.device = device
        self.name = name

    def snapshot(self) -> dict[str, str]:
        """Read the snippet's current semantic event snapshot.

        Returns:
            Event keys mapped to their latest semantic values.
        """
        return json.loads(self.device.nearby.snapshot())

    def reset(self) -> None:
        """Clear every active Nearby operation and recorded event."""
        self.device.nearby.reset()

    def wait_value(self, category: str, identifier: str, value: str) -> str:
        """Wait for one event key to equal the expected value.

        Returns:
            The matching event value.
        """
        key = f"{category}:{identifier}"

        def read() -> str | None:
            current = self.snapshot().get(key)
            return current if current == value else None

        return wait_for(f"{self.name} {key}={value}", read)

    def wait_prefix(self, category: str, identifier: str, prefix: str) -> str:
        """Wait for one event value to begin with the expected prefix.

        Returns:
            The matching event value.
        """
        key = f"{category}:{identifier}"

        def read() -> str | None:
            current = self.snapshot().get(key)
            if current is not None and current.startswith(prefix):
                return current
            return None

        return wait_for(f"{self.name} {key} prefix {prefix}", read)

    def wait_identifier(self, category: str, value: str) -> str:
        """Return the identifier of the matching categorized event.

        Returns:
            The matching event identifier.
        """
        key_prefix = f"{category}:"

        def read() -> str | None:
            for key, current in self.snapshot().items():
                if key.startswith(key_prefix) and current == value:
                    return key.removeprefix(key_prefix)
            return None

        return wait_for(f"{self.name} {category}={value}", read)

    def wait_operation(self, operation: str) -> None:
        """Wait for an asynchronous snippet operation to succeed."""
        self.wait_value("operation", operation, "succeeded")

    def send_bytes(self, endpoint: str, payload: bytes) -> int:
        """Send a bytes payload and return its Nearby identifier.

        Returns:
            The assigned Nearby payload identifier.
        """
        return self.device.nearby.sendBytes(endpoint, encode(payload))

    def send_file(self, endpoint: str, payload: bytes) -> int:
        """Send a file payload and return its Nearby identifier.

        Returns:
            The assigned Nearby payload identifier.
        """
        return self.device.nearby.sendFile(endpoint, encode(payload))

    def wait_payload(self, identifier: int, payload: bytes) -> None:
        """Wait for a payload with the expected identifier and digest."""
        self.wait_value("payload-sha256", str(identifier), sha256(payload))


@dataclass(frozen=True)
class AcceptedConnection:
    """Identifiers and authentication digits for an accepted peer pair."""

    initiator_endpoint: str
    responder_endpoint: str
    initiator_pin: str
    responder_pin: str


class NearbyPair:
    """Scenario operations involving an initiator and responder."""

    def __init__(self, initiator: Peer, responder: Peer) -> None:
        """Create a pair without starting discovery or advertising."""
        self.initiator = initiator
        self.responder = responder

    def begin_connection(self) -> tuple[str, str]:
        """Discover the responder and begin an unaccepted connection.

        Returns:
            Endpoint identifiers as observed by initiator and responder.
        """
        self.responder.device.nearby.startAdvertising(self.responder.name)
        self.responder.wait_operation("advertising")
        self.initiator.device.nearby.startDiscovery()
        self.initiator.wait_operation("discovery")
        responder_id = self.initiator.wait_identifier("discovered", "found")
        self.initiator.device.nearby.requestConnection(
            responder_id,
            self.initiator.name,
        )
        initiator_id = self.responder.wait_identifier(
            "connection",
            "initiated",
        )
        return responder_id, initiator_id

    def connect_and_accept(self) -> AcceptedConnection:
        """Connect both peers, exchange authentication, and accept.

        Returns:
            The accepted endpoints and both authentication observations.
        """
        responder_id, initiator_id = self.begin_connection()
        initiator_pin = self.initiator.wait_prefix(
            "authentication",
            responder_id,
            "",
        )
        responder_pin = self.responder.wait_prefix(
            "authentication",
            initiator_id,
            "",
        )
        self.initiator.device.nearby.acceptConnection(responder_id)
        self.responder.device.nearby.acceptConnection(initiator_id)
        self.initiator.wait_value("connection", responder_id, "connected")
        self.responder.wait_value("connection", initiator_id, "connected")
        return AcceptedConnection(
            initiator_endpoint=responder_id,
            responder_endpoint=initiator_id,
            initiator_pin=initiator_pin,
            responder_pin=responder_pin,
        )
