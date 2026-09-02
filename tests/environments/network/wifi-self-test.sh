#!/usr/bin/env bash
set -euo pipefail

mode=${1:?expected lan, hotspot-client, or hotspot-owner}
endpoint_ns=oqs-wifi-endpoint
peer_ns=oqs-wifi-peer
interfaces=($(iw dev | awk '$1 == "Interface" {print $2}' | sort))
hostapd_pid=/run/oqs-hostapd.pid
endpoint_pid=/run/oqs-wpa-endpoint.pid
peer_pid=/run/oqs-wpa-peer.pid

cleanup() {
  for pid_file in "${hostapd_pid}" "${endpoint_pid}" "${peer_pid}"; do
    if [[ -f ${pid_file} ]]; then
      kill "$(cat "${pid_file}")" 2>/dev/null || true
      rm -f "${pid_file}"
    fi
  done
  ip netns delete "${endpoint_ns}" 2>/dev/null || true
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

move_to_namespace() {
  local interface=$1
  local namespace=$2
  local phy="phy$(iw dev "${interface}" info | awk '$1 == "wiphy" {print $2}')"
  ip link set "${interface}" down
  ip netns add "${namespace}"
  iw phy "${phy}" set netns name "${namespace}"
  ip netns exec "${namespace}" ip link set lo up
}

write_hostapd_config() {
  local interface=$1
  cat > /tmp/oqs-hostapd.conf <<EOF
interface=${interface}
driver=nl80211
ssid=quickshare-reference
hw_mode=a
channel=36
wpa=2
wpa_key_mgmt=WPA-PSK
rsn_pairwise=CCMP
wpa_passphrase=quickshare-reference-passphrase
EOF
}

write_wpa_config() {
  local name=$1
  local control=$2
  cat >"/tmp/oqs-wpa-${name}.conf" <<EOF
ctrl_interface=${control}
network={
  ssid="quickshare-reference"
  psk="quickshare-reference-passphrase"
  key_mgmt=WPA-PSK
  scan_freq=5180
}
EOF
}

start_hostapd() {
  local namespace=$1
  local interface=$2
  write_hostapd_config "${interface}"
  run_in "${namespace}" hostapd -B -P "${hostapd_pid}" /tmp/oqs-hostapd.conf
}

start_station() {
  local namespace=$1
  local interface=$2
  local name=$3
  local pid_file=$4
  local control="/run/oqs-wpa-${name}"
  write_wpa_config "${name}" "${control}"
  run_in "${namespace}" wpa_supplicant -B -D nl80211 -i "${interface}" \
    -c "/tmp/oqs-wpa-${name}.conf" -P "${pid_file}"
  for _ in {1..100}; do
    if run_in "${namespace}" wpa_cli -p "${control}" -i "${interface}" status |
      grep -q '^wpa_state=COMPLETED$'; then
      return
    fi
    sleep 0.02
  done
  run_in "${namespace}" wpa_cli -p "${control}" -i "${interface}" status >&2
  exit 1
}

tcp_one_way() {
  local source_ns=$1
  local target_ns=$2
  local target_ip=$3
  local label=$4
  local ready="/run/oqs-tcp-${label}.ready"
  rm -f "${ready}"
  run_in "${target_ns}" /environment/tcp-roundtrip.py server "${target_ip}" "${ready}" &
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

verify_path() {
  local endpoint_location=$1
  local endpoint_ip=$2
  local peer_location=$3
  local peer_ip=$4
  run_in "${endpoint_location}" ping -q -c 2 -W 1 "${peer_ip}"
  run_in "${peer_location}" ping -q -c 2 -W 1 "${endpoint_ip}"
  tcp_one_way "${endpoint_location}" "${peer_location}" "${peer_ip}" outbound
  tcp_one_way "${peer_location}" "${endpoint_location}" "${endpoint_ip}" inbound
}

if [[ ${mode} == lan ]]; then
  [[ ${#interfaces[@]} -ge 3 ]] || { echo "LAN test needs three radios" >&2; exit 1; }
  endpoint=${interfaces[0]}
  peer=${interfaces[1]}
  access_point=${interfaces[2]}
  move_to_namespace "${endpoint}" "${endpoint_ns}"
  move_to_namespace "${peer}" "${peer_ns}"
  start_hostapd root "${access_point}"
  start_station "${endpoint_ns}" "${endpoint}" endpoint "${endpoint_pid}"
  start_station "${peer_ns}" "${peer}" peer "${peer_pid}"
  ip netns exec "${endpoint_ns}" ip address add 10.50.0.2/24 dev "${endpoint}"
  ip netns exec "${peer_ns}" ip address add 10.50.0.3/24 dev "${peer}"
  verify_path "${endpoint_ns}" 10.50.0.2 "${peer_ns}" 10.50.0.3
elif [[ ${mode} == hotspot-client ]]; then
  endpoint=${interfaces[0]}
  peer=${interfaces[1]}
  move_to_namespace "${peer}" "${peer_ns}"
  start_hostapd "${peer_ns}" "${peer}"
  start_station root "${endpoint}" endpoint "${endpoint_pid}"
  ip address add 10.51.0.2/24 dev "${endpoint}"
  ip netns exec "${peer_ns}" ip address add 10.51.0.1/24 dev "${peer}"
  verify_path root 10.51.0.2 "${peer_ns}" 10.51.0.1
elif [[ ${mode} == hotspot-owner ]]; then
  endpoint=${interfaces[0]}
  peer=${interfaces[1]}
  move_to_namespace "${peer}" "${peer_ns}"
  start_hostapd root "${endpoint}"
  start_station "${peer_ns}" "${peer}" peer "${peer_pid}"
  ip address add 10.52.0.1/24 dev "${endpoint}"
  ip netns exec "${peer_ns}" ip address add 10.52.0.2/24 dev "${peer}"
  verify_path root 10.52.0.1 "${peer_ns}" 10.52.0.2
else
  echo "unknown Wi-Fi self-test: ${mode}" >&2
  exit 2
fi

echo "${mode} association and bidirectional TCP self-test passed."
