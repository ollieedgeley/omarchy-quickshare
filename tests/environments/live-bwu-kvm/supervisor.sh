#!/bin/sh
set -eu

for attempt in $(seq 1 100); do
  [ -S /runtime/bt-server-bredrle ] && break
  sleep 0.02
done
[ -S /runtime/bt-server-bredrle ]
python3 /environment/h4_relay.py &
relay_pid=$!
for attempt in $(seq 1 100); do
  [ -S /runtime/h4-relay.sock ] && break
  sleep 0.02
done
[ -S /runtime/h4-relay.sock ]

cleanup() {
  kill "$relay_pid" 2>/dev/null || true
  wait "$relay_pid" 2>/dev/null || true
  kill "$guest_a_pid" "$guest_b_pid" 2>/dev/null || true
  wait "$guest_a_pid" "$guest_b_pid" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

qemu() {
  peer=$1
  lan=$2
  kernel_args="console=ttyS0,115200 root=oqs-root rootfstype=9p "
  kernel_args="${kernel_args}rootflags=trans=virtio,version=9p2000.u ro "
  kernel_args="${kernel_args}init=/environment/guest-init.sh oqs.peer=$peer"
  case "$peer" in
    a) mac=01 ;;
    b) mac=02 ;;
  esac
  qemu-system-x86_64 -machine q35,accel=kvm -cpu host -m 512M -smp 1 \
    -nodefaults -no-reboot -display none -monitor none \
    -kernel /boot/vmlinuz-oqs -initrd /boot/initrd-oqs \
    -append "$kernel_args" \
    -fsdev local,id=root,path=/,security_model=none,multidevs=remap,\
readonly=on \
    -device virtio-9p-pci,fsdev=root,mount_tag=oqs-root \
    -device virtio-serial-pci \
    -chardev socket,id=control,path=/runtime/$peer.control.sock,\
server=on,wait=off \
    -device virtserialport,chardev=control,name=oqs.control \
    -chardev socket,id=hci,path=/runtime/h4-relay.sock,reconnect=5 \
    -device isa-serial,chardev=hci \
    -netdev socket,id=lan,$lan \
    -device e1000,netdev=lan,mac=52:54:00:00:00:$mac \
    -serial file:/runtime/$peer.console.log
}

qemu a listen=127.0.0.1:45551 &
guest_a_pid=$!
if [ "${OQS_PEERS:-two}" = two ]; then
  sleep 1
  qemu b connect=127.0.0.1:45551 &
  guest_b_pid=$!
  wait "$guest_a_pid" "$guest_b_pid"
else
  guest_b_pid=$guest_a_pid
  wait "$guest_a_pid"
fi
