#!/usr/bin/env python3
import socket
import subprocess
import sys
import time

LEFT = "oqs-netem-left"
RIGHT = "oqs-netem-right"
SCRIPT = "/environment/netem-self-test.py"
PORT = 28431
PAYLOAD = b"quickshare-netem-control"


def run(*args: str, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(args, check=check, text=True, capture_output=True)


def server() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("10.23.0.2", PORT))
    while True:
        payload, peer = sock.recvfrom(4096)
        sock.sendto(payload, peer)


def client(expect_reply: bool) -> None:
    result = run("ip", "netns", "exec", LEFT, SCRIPT, "client", check=False)
    if (result.returncode == 0) != expect_reply:
        raise RuntimeError(
            f"UDP reply expectation {expect_reply} failed: "
            f"{result.stdout}{result.stderr}"
        )


def client_process() -> None:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(0.4)
    sock.sendto(PAYLOAD, ("10.23.0.2", PORT))
    payload, _ = sock.recvfrom(4096)
    if payload != PAYLOAD:
        raise RuntimeError("netem control corrupted UDP payload")


def configure_namespace(namespace: str, interface: str, address: str) -> None:
    prefix = ("ip", "netns", "exec", namespace, "ip")
    run(*prefix, "link", "set", "lo", "up")
    run(*prefix, "link", "set", interface, "up")
    run(*prefix, "address", "add", address, "dev", interface)


def setup_network() -> None:
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
    client(True)
    run(
        "ip", "netns", "exec", LEFT, "tc", "qdisc", "add",
        "dev", "netem-left", "root", "netem", "loss", "100%",
    )
    client(False)
    run(
        "ip", "netns", "exec", LEFT, "tc", "qdisc", "del",
        "dev", "netem-left", "root",
    )
    client(True)


def main() -> None:
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
