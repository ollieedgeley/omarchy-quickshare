"""Bridge one QEMU stream NIC to the container's private Ethernet interface."""  # ruff: ignore[implicit-namespace-package] - Standalone container entrypoint.

import selectors
import socket
import struct

PORT = 19090
MAX_FRAME = 65535
ETHERNET_HEADER = 14
PACKET_OUTGOING = 4
SOL_PACKET = 263
PACKET_ADD_MEMBERSHIP = 1
PACKET_MR_PROMISC = 1


def _exact(connection: socket.socket, size: int) -> bytes:
    data = bytearray()
    while len(data) < size:
        chunk = connection.recv(size - len(data))
        if not chunk:
            raise EOFError
        data.extend(chunk)
    return bytes(data)


def _forward(connection: socket.socket, ethernet: socket.socket) -> None:
    connection.settimeout(5)
    with selectors.DefaultSelector() as ready:
        ready.register(connection, selectors.EVENT_READ)
        ready.register(ethernet, selectors.EVENT_READ)
        while True:
            for key, _ in ready.select():
                if key.fileobj is connection:
                    size = struct.unpack("!I", _exact(connection, 4))[0]
                    if not ETHERNET_HEADER <= size <= MAX_FRAME:
                        message = f"Invalid QEMU Ethernet frame length: {size}"
                        raise ValueError(message)
                    ethernet.send(_exact(connection, size))
                else:
                    frame, address = ethernet.recvfrom(MAX_FRAME)
                    if address[2] != PACKET_OUTGOING:
                        connection.sendall(
                            struct.pack("!I", len(frame)) + frame
                        )


def main() -> None:
    """Serve bounded, reconnectable Ethernet streams without host privileges."""
    with (
        socket.socket(
            socket.AF_PACKET, socket.SOCK_RAW, socket.htons(3)
        ) as ethernet,
        socket.socket() as listener,
    ):
        ethernet.bind(("eth0", 0))
        membership = struct.pack(
            "IHH8s", socket.if_nametoindex("eth0"), PACKET_MR_PROMISC, 0, b""
        )
        ethernet.setsockopt(SOL_PACKET, PACKET_ADD_MEMBERSHIP, membership)
        ethernet.settimeout(5)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        # Only the owned internal bridge is attached; QEMU connects from host.
        listener.bind(("0.0.0.0", PORT))  # ruff: ignore[hardcoded-bind-all-interfaces] - Sole owned internal Docker network.
        listener.listen(1)
        print("Ethernet relay ready on eth0", flush=True)
        while True:
            connection, address = listener.accept()
            print(f"QEMU stream connected: {address}", flush=True)
            with connection:
                try:
                    _forward(connection, ethernet)
                except (EOFError, OSError, ValueError) as error:
                    print(f"QEMU stream closed: {error}", flush=True)


if __name__ == "__main__":
    main()
