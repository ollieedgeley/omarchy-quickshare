#!/usr/bin/env python3
"""Provide a small TCP round-trip peer for virtual-network gates."""

import socket
import sys
from pathlib import Path

PORT = 28432


def server(address: str, ready: str) -> None:
    """Echo one TCP stream after publishing a readiness marker."""
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind((address, PORT))
    listener.listen(1)
    Path(ready).write_text("ready\n", encoding="utf-8")
    connection, _ = listener.accept()
    chunks = []
    while chunk := connection.recv(4096):
        chunks.append(chunk)
    connection.sendall(b"".join(chunks))
    connection.close()
    listener.close()


def client(address: str, payload: str) -> None:
    """Send one payload and require a byte-identical echo.

    Raises:
        RuntimeError: If the server returns different bytes.
    """
    expected = payload.encode()
    connection = socket.create_connection((address, PORT), timeout=2)
    connection.sendall(expected)
    connection.shutdown(socket.SHUT_WR)
    received = bytearray()
    while chunk := connection.recv(4096):
        received.extend(chunk)
    connection.close()
    if bytes(received) != expected:
        message = (
            f"TCP echo returned {len(received)} bytes, expected {len(expected)}"
        )
        raise RuntimeError(message)


if __name__ == "__main__":
    if sys.argv[1] == "server":
        server(sys.argv[2], sys.argv[3])
    elif sys.argv[1] == "client":
        client(sys.argv[2], sys.argv[3])
    else:
        message = "expected server or client"
        raise RuntimeError(message)
