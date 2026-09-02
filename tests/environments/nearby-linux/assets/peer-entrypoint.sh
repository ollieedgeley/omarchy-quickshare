#!/bin/sh
set -eu

mkdir -p /run/dbus /run/avahi-daemon
if [ -d /cases ]; then
  for directory in outbound received; do
    test -d "/cases/$directory"
  done
fi
install -d -o quickshare -g quickshare -m 0700 /run/quickshare
runuser --user quickshare -- test -w /run/quickshare
runuser --user quickshare -- test -w /cases/outbound
runuser --user quickshare -- test -w /cases/received
dbus-daemon --system --fork --nopidfile
python3 -m dbusmock --system --template bluez5 >/run/bluez5.log 2>&1 &
bluez_pid=$!
NetworkManager --no-daemon >/run/network-manager.log 2>&1 &
network_manager_pid=$!
avahi-daemon --no-chroot --no-drop-root >/run/avahi-daemon.log 2>&1 &
avahi_pid=$!

cleanup() {
  kill "$avahi_pid" "$network_manager_pid" "$bluez_pid" 2>/dev/null || true
}

trap cleanup EXIT INT TERM

wait_for_bus_name() {
  destination=$1
  until dbus-send --system --print-reply --dest="$destination" / \
    org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; do
    sleep 0.1
  done
}

wait_for_bus_name org.bluez
wait_for_bus_name org.freedesktop.NetworkManager
dbus-send --system --print-reply --dest=org.bluez / org.bluez.Mock.AddAdapter \
  string:hci0 string:"${HOSTNAME}" >/dev/null
until dbus-send --system --print-reply --dest=org.bluez /org/bluez/hci0 \
  org.freedesktop.DBus.Properties.Get string:org.bluez.Adapter1 \
  string:Powered >/dev/null 2>&1; do
  sleep 0.1
done
nmcli device set eth0 managed yes || true

if [ "$#" -eq 0 ]; then
  wait "$bluez_pid" "$network_manager_pid" "$avahi_pid"
else
  exec runuser --user quickshare -- "$@"
fi
