#!/usr/bin/python3
"""Exercise a Bumble RFCOMM peer through the Linux Bluetooth stack."""

import socket
import time

PEER_ADDRESS = "00:AA:01:01:00:01"
BLUEZ_TO_BUMBLE = b"bluez-to-bumble-classic"
BUMBLE_TO_BLUEZ = b"bumble-to-bluez-classic"


def connect_tcp():
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        connection = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        connection.settimeout(1)
        try:
            connection.connect(("127.0.0.1", 8888))
            return connection
        except OSError:
            connection.close()
            time.sleep(0.05)
    raise TimeoutError("timed out waiting for Bumble TCP bridge")


def main():
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
            raise AssertionError("RFCOMM-to-Bumble payload changed")

        tcp.sendall(BUMBLE_TO_BLUEZ)
        if rfcomm.recv(4096) != BUMBLE_TO_BLUEZ:
            raise AssertionError("Bumble-to-RFCOMM payload changed")
    finally:
        rfcomm.close()
        tcp.close()

    print("BLUEZ_RFCOMM_BIDIRECTIONAL_OK", flush=True)


if __name__ == "__main__":
    main()
