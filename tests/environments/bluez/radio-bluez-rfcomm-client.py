#!/usr/bin/python3
"""Exercise a Bumble RFCOMM peer through the Linux Bluetooth stack."""

import socket
import time

PEER_ADDRESS = "00:AA:01:01:00:01"
BLUEZ_TO_BUMBLE = b"bluez-to-bumble-classic"
BUMBLE_TO_BLUEZ = b"bumble-to-bluez-classic"


def connect_tcp() -> socket.socket:
    """Connect to the Bumble peer's fixture TCP bridge.

    Returns:
        The connected control socket.

    Raises:
        TimeoutError: If the fixture bridge does not become ready.
    """
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        connection = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        connection.settimeout(1)
        try:
            connection.connect(("127.0.0.1", 8888))
        except OSError:
            connection.close()
            time.sleep(0.05)
        else:
            return connection
    message = "timed out waiting for Bumble TCP bridge"
    raise TimeoutError(message)


def main() -> None:
    """Run a bidirectional RFCOMM exchange through the Bumble peer.

    Raises:
        AssertionError: If either transport changes its fixture payload.
    """
    tcp = connect_tcp()
    rfcomm = socket.socket(
        socket.AF_BLUETOOTH, socket.SOCK_STREAM, socket.BTPROTO_RFCOMM
    )
    tcp.settimeout(5)
    rfcomm.settimeout(5)
    try:
        rfcomm.connect((PEER_ADDRESS, 1))

        rfcomm.sendall(BLUEZ_TO_BUMBLE)
        if tcp.recv(4096) != BLUEZ_TO_BUMBLE:
            message = "RFCOMM-to-Bumble payload changed"
            raise AssertionError(message)

        tcp.sendall(BUMBLE_TO_BLUEZ)
        if rfcomm.recv(4096) != BUMBLE_TO_BLUEZ:
            message = "Bumble-to-RFCOMM payload changed"
            raise AssertionError(message)
    finally:
        rfcomm.close()
        tcp.close()

    print("BLUEZ_RFCOMM_BIDIRECTIONAL_OK", flush=True)


if __name__ == "__main__":
    main()
