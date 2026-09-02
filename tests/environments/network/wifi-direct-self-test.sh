#!/usr/bin/env bash
set -euo pipefail

peer_ns=oqs-p2p-peer
endpoint_control=/run/oqs-p2p-endpoint
peer_control=/run/oqs-p2p-peer
endpoint_pid=/run/oqs-p2p-endpoint.pid
peer_pid=/run/oqs-p2p-peer.pid
interfaces=($(iw dev | awk '$1 == "Interface" {print $2}' | sort))

cleanup() {
  for pid_file in "${endpoint_pid}" "${peer_pid}"; do
    if [[ -f ${pid_file} ]]; then
      kill "$(cat "${pid_file}")" 2>/dev/null || true
      rm -f "${pid_file}"
    fi
  done
  ip netns delete "${peer_ns}" 2>/dev/null || true
}
trap cleanup EXIT

run_in() {
  local namespace=$1
  shift
  if [[ ${namespace} == root ]]; then
    "$@"
  else
    ip netns exec "${namespace}" "$@"
  fi
}

wpa() {
  local namespace=$1
  local control=$2
  local interface=$3
  shift 3
  run_in "${namespace}" wpa_cli -p "${control}" -i "${interface}" "$@"
}

expect_ok() {
  local output
  output=$("$@")
  [[ ${output} == OK* ]] || {
    printf 'command failed: %s\n' "${output}" >&2
    exit 1
  }
}

write_config() {
  local name=$1
  local control=$2
  cat >"/tmp/oqs-p2p-${name}.conf" <<EOF
ctrl_interface=${control}
device_name=quickshare-${name}
device_type=1-0050F204-1
config_methods=display push_button keypad
p2p_go_intent=7
EOF
}

move_peer() {
  local interface=$1
  local phy="phy$(iw dev "${interface}" info | awk '$1 == "wiphy" {print $2}')"
  ip link set "${interface}" down
  ip netns add "${peer_ns}"
  iw phy "${phy}" set netns name "${peer_ns}"
  ip netns exec "${peer_ns}" ip link set lo up
}

start_supplicant() {
  local namespace=$1
  local interface=$2
  local name=$3
  local control=$4
  local pid_file=$5
  write_config "${name}" "${control}"
  run_in "${namespace}" wpa_supplicant -B -D nl80211 -i "${interface}" \
    -c "/tmp/oqs-p2p-${name}.conf" -P "${pid_file}"
}

group_interface() {
  local namespace=$1
  local kind=$2
  run_in "${namespace}" iw dev |
    awk -v kind="${kind}" \
      '$1 == "Interface" {interface=$2}
       $1 == "type" && $2 == kind {print interface; exit}'
}

wait_for_group() {
  local namespace=$1
  local kind=$2
  local interface
  for _ in {1..250}; do
    interface=$(group_interface "${namespace}" "${kind}")
    if [[ -n ${interface} ]]; then
      printf '%s\n' "${interface}"
      return
    fi
    sleep 0.02
  done
  run_in "${namespace}" iw dev >&2
  exit 1
}

tcp_one_way() {
  local source_ns=$1
  local target_ns=$2
  local target_ip=$3
  local label=$4
  local ready="/run/oqs-p2p-tcp-${label}.ready"
  rm -f "${ready}"
  run_in "${target_ns}" /environment/tcp-roundtrip.py \
    server "${target_ip}" "${ready}" &
  local server_pid=$!
  for _ in {1..100}; do
    [[ -f ${ready} ]] && break
    sleep 0.01
  done
  [[ -f ${ready} ]] || { echo "TCP server did not become ready" >&2; exit 1; }
  run_in "${source_ns}" /environment/tcp-roundtrip.py client "${target_ip}" \
    "quickshare-${label}-payload"
  wait "${server_pid}"
  rm -f "${ready}"
}

if [[ ${#interfaces[@]} -lt 2 ]]; then
  echo "Wi-Fi Direct test needs two radios" >&2
  exit 1
fi
endpoint=${interfaces[0]}
peer=${interfaces[1]}
move_peer "${peer}"
start_supplicant \
  root "${endpoint}" endpoint "${endpoint_control}" "${endpoint_pid}"
start_supplicant "${peer_ns}" "${peer}" peer "${peer_control}" "${peer_pid}"

peer_address=$(wpa "${peer_ns}" "${peer_control}" "${peer}" status |
  awk -F= '$1 == "p2p_device_address" {print $2}')
if [[ -z ${peer_address} ]]; then
  echo "peer lacks a P2P device address" >&2
  exit 1
fi
expect_ok wpa "${peer_ns}" "${peer_control}" "${peer}" p2p_group_add freq=2412
peer_group=$(wait_for_group "${peer_ns}" P2P-GO)
expect_ok wpa "${peer_ns}" "${peer_control}" "${peer_group}" wps_pbc

expect_ok wpa root "${endpoint_control}" "${endpoint}" p2p_find
discovered=false
for _ in {1..250}; do
  if wpa root "${endpoint_control}" "${endpoint}" p2p_peers |
    grep -Fqx "${peer_address}"; then
    discovered=true
    break
  fi
  sleep 0.02
done
if [[ ${discovered} != true ]]; then
  echo "remote group owner was not discovered" >&2
  exit 1
fi
expect_ok wpa root "${endpoint_control}" "${endpoint}" p2p_connect \
  "${peer_address}" pbc join freq=2412
endpoint_group=$(wait_for_group root P2P-client)

ip address add 10.53.0.2/24 dev "${endpoint_group}"
ip netns exec "${peer_ns}" ip address add 10.53.0.1/24 dev "${peer_group}"
run_in root ping -q -c 2 -W 1 10.53.0.1
run_in "${peer_ns}" ping -q -c 2 -W 1 10.53.0.2
tcp_one_way root "${peer_ns}" 10.53.0.1 outbound
tcp_one_way "${peer_ns}" root 10.53.0.2 inbound

echo "Wi-Fi Direct remote-owner/client association and bidirectional TCP " \
  "self-test passed."
