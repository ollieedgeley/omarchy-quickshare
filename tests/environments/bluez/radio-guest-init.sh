#!/bin/bash
set -euo pipefail

export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PYTHONPATH=/typing-extensions/src:/bumble

mountpoint -q /proc || mount -t proc proc /proc
mountpoint -q /sys || mount -t sysfs sysfs /sys
mountpoint -q /dev || mount -t devtmpfs devtmpfs /dev
mkdir -p /dev/pts /run /tmp /var/lib/bluetooth
mountpoint -q /dev/pts || mount -t devpts devpts /dev/pts
mountpoint -q /run || mount -t tmpfs -o mode=755,nosuid,nodev tmpfs /run
mountpoint -q /tmp || mount -t tmpfs -o mode=1777,nosuid,nodev tmpfs /tmp
mountpoint -q /var/lib/bluetooth \
  || mount -t tmpfs -o mode=700,nosuid,nodev tmpfs /var/lib/bluetooth
mkdir -p /run/dbus
busybox ip link set lo up

modprobe hci_vhci
modprobe virtio_console
/artifacts/btvirt -l2 >/run/btvirt.log 2>&1 &
controller_count() {
  find /sys/class/bluetooth -maxdepth 1 -name 'hci*' 2>/dev/null |
    wc -l
}
for _ in {1..200}; do
  [[ $(controller_count) -eq 2 ]] && break
  sleep 0.02
done
[[ $(controller_count) -eq 2 ]]
btmon --write /runtime/radio.btsnoop >/run/btmon.log 2>&1 &

dbus-daemon --system --fork --nopidfile
bluetoothd --nodetach --debug >/run/bluetoothd.log 2>&1 &
for _ in {1..200}; do
  bluetoothctl list | grep -q '^Controller ' && break
  sleep 0.02
done
[[ $(bluetoothctl list | grep -c '^Controller ') -eq 2 ]]

run_ble() {
  btmgmt -i hci1 power off
  python3 /environment/radio-bumble-gatt-peer.py >/run/bumble-gatt.log 2>&1 &
  local peer=$!
  for _ in {1..200}; do
    grep -q '^GATT_READY$' /run/bumble-gatt.log && break
    kill -0 "${peer}" 2>/dev/null || break
    sleep 0.02
  done
  if ! grep -q '^GATT_READY$' /run/bumble-gatt.log; then
    cat /run/bumble-gatt.log
    kill "${peer}" 2>/dev/null || true
    wait "${peer}" 2>/dev/null || true
    return 1
  fi

  local status=0
  python3 /environment/radio-bluez-gatt-client.py || status=$?
  sleep 0.1
  kill "${peer}" 2>/dev/null || true
  wait "${peer}" 2>/dev/null || true
  cat /run/bumble-gatt.log
  [[ ${status} -eq 0 ]]
  grep -q '^GATT_READ 62756d626c652d746f2d626c75657a$' /run/bumble-gatt.log
  grep -q '^GATT_WRITE 626c75657a2d746f2d62756d626c65$' /run/bumble-gatt.log
}

run_classic() {
  btmgmt -i hci1 power off
  PYTHONUNBUFFERED=1 python3 /bumble/examples/run_rfcomm_server.py \
    /bumble/examples/device1.json hci-socket:1 8888 \
    >/run/bumble-rfcomm.log 2>&1 &
  local peer=$!
  for _ in {1..200}; do
    grep -q 'Listening for RFComm connections on channel 1' \
      /run/bumble-rfcomm.log && break
    kill -0 "${peer}" 2>/dev/null || break
    sleep 0.02
  done
  if ! grep -q 'Listening for RFComm connections on channel 1' \
    /run/bumble-rfcomm.log; then
    cat /run/bumble-rfcomm.log
    kill "${peer}" 2>/dev/null || true
    wait "${peer}" 2>/dev/null || true
    return 1
  fi

  local status=0
  python3 /environment/radio-bluez-rfcomm-client.py || status=$?
  kill "${peer}" 2>/dev/null || true
  wait "${peer}" 2>/dev/null || true
  grep -E \
    'Starting TCP|Listening for RFComm|RFComm session|RFCOMM Data|TCP Server' \
    /run/bumble-rfcomm.log || true
  [[ ${status} -eq 0 ]]
}

control=
for candidate in /sys/class/virtio-ports/*; do
  if [[ $(<"${candidate}/name") == oqs.control ]]; then
    control="/dev/${candidate##*/}"
    break
  fi
done
[[ -n ${control} ]]

while true; do
  exec 3<>"${control}"
  printf 'READY\n' >&3
  while IFS= read -r command <&3; do
    case "${command}" in
      RUN_CONTROLLER)
        set +e
        output=$(bluetoothctl list 2>&1)
        status=$?
        set -e
        while IFS= read -r line; do
          printf 'OUT %s\n' "${line}" >&3
        done <<<"${output}"
        printf 'STATUS %s\n' "${status}" >&3
        ;;
      RUN_BLE)
        set +e
        output=$(run_ble 2>&1)
        status=$?
        set -e
        while IFS= read -r line; do
          printf 'OUT %s\n' "${line}" >&3
        done <<<"${output}"
        printf 'STATUS %s\n' "${status}" >&3
        ;;
      RUN_CLASSIC)
        set +e
        output=$(run_classic 2>&1)
        status=$?
        set -e
        while IFS= read -r line; do
          printf 'OUT %s\n' "${line}" >&3
        done <<<"${output}"
        printf 'STATUS %s\n' "${status}" >&3
        ;;
      STOP)
        printf 'STOPPING\n' >&3
        sync
        busybox poweroff -f
        ;;
      *)
        printf 'OUT unknown guest command: %s\nSTATUS 2\n' "${command}" >&3
        ;;
    esac
  done
  exec 3>&-
done
