#!/bin/sh
set -eu

peer=$(sed -n 's/.*oqs.peer=\([^ ]*\).*/\1/p' /proc/cmdline)
case "$peer" in
  a) address=192.0.2.1 ;;
  b) address=192.0.2.2 ;;
  *) echo "unknown peer" >&2; exit 1 ;;
esac

mount -t proc proc /proc || true
mount -t sysfs sys /sys || true
mount -t devtmpfs dev /dev || true
modprobe virtio_console
network=
for candidate in /sys/class/net/*; do
  name=${candidate##*/}
  [ "$name" = lo ] || network=$name
done
[ -n "$network" ]
busybox ip link set "$network" up
busybox ip address add "$address/30" dev "$network"
btattach -B /dev/ttyS1 -P h4 -N >/run/btattach.log 2>&1 &
mkdir -p /run/dbus
dbus-daemon --system --fork
bluetoothd --nodetach --debug --experimental >/run/bluetoothd.log 2>&1 &
exec python3 /environment/guest_peer.py "$peer"
