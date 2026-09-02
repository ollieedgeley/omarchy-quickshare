"""Exercise the public Nearby Connections API between two Android peers."""

import signal
import time
from types import FrameType

from mobly import asserts, base_test, test_runner
from mobly.controllers import android_device
from nearby_pair import AcceptedConnection, NearbyPair, Peer

PROBE_PACKAGE = "dev.omarchy.quickshare.probe"
PROTOCOL_TIMEOUT_SECONDS = 60


def fail_protocol_timeout(
    _signal_number: int,
    _frame: FrameType | None,
) -> None:
    """Fail a protocol case when its deterministic alarm expires.

    Raises:
        TimeoutError: Always, because the configured alarm expired.
    """
    message = "Android Nearby protocol case exceeded its budget"
    raise TimeoutError(message)


class NearbyConnectionsTest(base_test.BaseTestClass):
    """Mobly cases for bidirectional public Nearby Connections behavior."""

    def setup_class(self) -> None:
        """Register both emulator peers and load the control snippet."""
        devices = self.register_controller(android_device, min_number=2)
        peer_a = android_device.get_device(devices, label="peer_a")
        peer_b = android_device.get_device(devices, label="peer_b")
        peer_a.load_snippet("nearby", PROBE_PACKAGE)
        peer_b.load_snippet("nearby", PROBE_PACKAGE)
        self.peer_a = Peer(peer_a, "peer-a")
        self.peer_b = Peer(peer_b, "peer-b")
        asserts.assert_equal(self.peer_a.device.nearby.ping(), "ready")
        asserts.assert_equal(self.peer_b.device.nearby.ping(), "ready")

    def setup_test(self) -> None:
        """Reset peer state and start the per-case time budget."""
        self.protocol_started_at = time.monotonic()
        signal.signal(signal.SIGALRM, fail_protocol_timeout)
        signal.alarm(PROTOCOL_TIMEOUT_SECONDS)
        self.peer_a.reset()
        self.peer_b.reset()

    def teardown_test(self) -> None:
        """Reset both peers and prove the case stayed within its budget."""
        try:
            self.peer_a.reset()
            self.peer_b.reset()
        finally:
            signal.alarm(0)
        elapsed = time.monotonic() - self.protocol_started_at
        asserts.assert_less_equal(elapsed, PROTOCOL_TIMEOUT_SECONDS)

    def accepted_pair(self) -> tuple[NearbyPair, AcceptedConnection]:
        """Connect both peers and confirm their authentication digits match.

        Returns:
            The peer helper and accepted connection observations.
        """
        pair = NearbyPair(self.peer_a, self.peer_b)
        connection = pair.connect_and_accept()
        asserts.assert_equal(
            connection.initiator_pin,
            connection.responder_pin,
        )
        return pair, connection

    def test_bytes_move_both_directions(self) -> None:
        """Move a bytes payload from each peer to the other."""
        pair, connection = self.accepted_pair()
        outbound = b"quickshare bytes from peer a"
        identifier = pair.initiator.send_bytes(
            connection.initiator_endpoint,
            outbound,
        )
        pair.responder.wait_payload(identifier, outbound)
        inbound = b"quickshare bytes from peer b"
        identifier = pair.responder.send_bytes(
            connection.responder_endpoint,
            inbound,
        )
        pair.initiator.wait_payload(identifier, inbound)

    def test_files_move_both_directions(self) -> None:
        """Move a file payload from each peer to the other."""
        pair, connection = self.accepted_pair()
        outbound = b"a" * 65_537 + b"outbound"
        identifier = pair.initiator.send_file(
            connection.initiator_endpoint,
            outbound,
        )
        pair.responder.wait_payload(identifier, outbound)
        inbound = b"b" * 65_539 + b"inbound"
        identifier = pair.responder.send_file(
            connection.responder_endpoint,
            inbound,
        )
        pair.initiator.wait_payload(identifier, inbound)

    def test_responder_rejects_connection(self) -> None:
        """Expose a responder rejection to the initiating peer."""
        pair = NearbyPair(self.peer_a, self.peer_b)
        responder_id, initiator_id = pair.begin_connection()
        self.peer_b.device.nearby.rejectConnection(initiator_id)
        self.peer_b.wait_operation(f"reject:{initiator_id}")
        self.peer_a.wait_prefix("connection", responder_id, "failed:")


if __name__ == "__main__":
    test_runner.main()
