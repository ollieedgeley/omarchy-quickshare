#!/usr/bin/env bash
set -euo pipefail

peer_namespace=oqs-radio-peer
state=/run/oqs-radio.state

setup() {
  interfaces=($(iw dev | awk '$1 == "Interface" {print $2}' | sort))
  if [[ ${#interfaces[@]} -lt 2 ]]; then
    echo "wmediumd self-test needs two radios" >&2
    exit 1
  fi
  first=${interfaces[0]}
  second=${interfaces[1]}
  second_phy="phy$(iw dev "${second}" info | awk '$1 == "wiphy" {print $2}')"

  ip link set "${first}" down
  ip link set address 42:00:00:00:00:00 dev "${first}"
  ip link set "${second}" down
  ip link set address 42:00:00:00:01:00 dev "${second}"
  ip netns add "${peer_namespace}"
  iw phy "${second_phy}" set netns name "${peer_namespace}"

  ip link set lo up
  iw dev "${first}" set type mp
  ip link set "${first}" up
  iw dev "${first}" mesh join quickshare-mesh freq 5180
  ip address add 10.10.10.10/24 dev "${first}"

  ip netns exec "${peer_namespace}" ip link set lo up
  ip netns exec "${peer_namespace}" iw dev "${second}" set type mp
  ip netns exec "${peer_namespace}" ip link set "${second}" up
  ip netns exec "${peer_namespace}" iw dev "${second}" mesh join quickshare-mesh freq 5180
  ip netns exec "${peer_namespace}" ip address add 10.10.10.11/24 dev "${second}"
  printf '%s\n' "${first}" "${second}" >"${state}"
}

control() {
  ping -q -c 2 -W 1 10.10.10.11
  ip netns exec "${peer_namespace}" ping -q -c 2 -W 1 10.10.10.10
}

fault() {
  if ping -q -c 1 -W 1 10.10.10.11; then
    echo "wmediumd drop model did not isolate the outbound radio" >&2
    exit 1
  fi
  if ip netns exec "${peer_namespace}" ping -q -c 1 -W 1 10.10.10.10; then
    echo "wmediumd drop model did not isolate the inbound radio" >&2
    exit 1
  fi
}

cleanup() {
  ip netns delete "${peer_namespace}" 2>/dev/null || true
  rm -f "${state}"
}

case "${1:-}" in
  setup) setup ;;
  control) control ;;
  fault) fault ;;
  cleanup) cleanup ;;
  *) echo "expected setup, control, fault, or cleanup" >&2; exit 2 ;;
esac
