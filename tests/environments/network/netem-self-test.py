#!/usr/bin/env python3
"""Prove deterministic UDP loss and recovery through Linux netem."""

import socket
import subprocess
import sys
import time

LEFT = "oqs-netem-left"
RIGHT = "oqs-netem-right"
SCRIPT = "/environment/netem-self-test.py"
PORT = 28431
PAYLOAD = b"quickshare-netem-control"


def run(*args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    """Run one fixed test-network command and capture its result.

    Returns:
        The completed command result.
    """
    return subprocess.run(args, check=check, text=True, capture_output=True)


def server() -> None:
    """Echo UDP datagrams until the parent test process terminates."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("10.23.0.2", PORT))
    while True:
        payload, peer = sock.recvfrom(4096)
        sock.sendto(payload, peer)


def client(*, expect_reply: bool) -> None:
    """Run the client and check whether a reply should cross netem.

    Raises:
        RuntimeError: If the observed outcome differs from the expectation.
    """
    result = run("ip", "netns", "exec", LEFT, SCRIPT, "client", check=False)
    if (result.returncode == 0) != expect_reply:
        message = (
            f"UDP reply expectation {expect_reply} failed: "
            f"{result.stdout}{result.stderr}"
        )
        raise RuntimeError(message)


def client_process() -> None:
    """Send and verify one UDP control payload.

    Raises:
        RuntimeError: If the echoed payload differs from the sent bytes.
    """
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(0.4)
    sock.sendto(PAYLOAD, ("10.23.0.2", PORT))
    payload, _ = sock.recvfrom(4096)
    if payload != PAYLOAD:
        message = "netem control corrupted UDP payload"
        raise RuntimeError(message)


def configure_namespace(namespace: str, interface: str, address: str) -> None:
    """Bring up one isolated network namespace and its test address."""
    prefix = ("ip", "netns", "exec", namespace, "ip")
    run(*prefix, "link", "set", "lo", "up")
    run(*prefix, "link", "set", interface, "up")
    run(*prefix, "address", "add", address, "dev", interface)


def setup_network() -> None:
    """Create the namespace pair and connect it with a veth link."""
    run("ip", "netns", "add", LEFT)
    run("ip", "netns", "add", RIGHT)
    run(
        "ip",
        "link",
        "add",
        "netem-left",
        "type",
        "veth",
        "peer",
        "name",
        "netem-right",
    )
    run("ip", "link", "set", "netem-left", "netns", LEFT)
    run("ip", "link", "set", "netem-right", "netns", RIGHT)
    configure_namespace(LEFT, "netem-left", "10.23.0.1/24")
    configure_namespace(RIGHT, "netem-right", "10.23.0.2/24")


def exercise_faults() -> None:
    """Prove control, total loss, and recovery in sequence."""
    client(expect_reply=True)
    run(
        "ip",
        "netns",
        "exec",
        LEFT,
        "tc",
        "qdisc",
        "add",
        "dev",
        "netem-left",
        "root",
        "netem",
        "loss",
        "100%",
    )
    client(expect_reply=False)
    run(
        "ip",
        "netns",
        "exec",
        LEFT,
        "tc",
        "qdisc",
        "del",
        "dev",
        "netem-left",
        "root",
    )
    client(expect_reply=True)


def main() -> None:
    """Run the complete netem self-test with unconditional cleanup."""
    peer = None
    try:
        setup_network()
        peer = subprocess.Popen(
            ["ip", "netns", "exec", RIGHT, SCRIPT, "server"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        time.sleep(0.05)
        exercise_faults()
    finally:
        if peer is not None:
            peer.terminate()
            peer.wait(timeout=2)
        run("ip", "netns", "delete", LEFT, check=False)
        run("ip", "netns", "delete", RIGHT, check=False)
    print("netem UDP control-fault-control self-test passed.")


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "server":
        server()
    elif len(sys.argv) > 1 and sys.argv[1] == "client":
        client_process()
    else:
        main()
