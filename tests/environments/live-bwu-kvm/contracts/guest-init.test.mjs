import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../guest-init.sh", import.meta.url),
  "utf8",
);
const VIRTIO = /^modprobe virtio_console$/mu;
const CANDIDATES = /^for candidate in \/sys\/class\/net\/\*; do$/mu;
const NETWORK_UP = /^busybox ip link set "\$network" up$/mu;
const NETWORK_ADDRESS =
  /^busybox ip address add "\$address\/30" dev "\$network"$/mu;
const BTATTACH =
  /^btattach -B \/dev\/ttyS1 -P h4 -N >\/run\/btattach\.log 2>&1 &$/mu;
const PEER = /^exec python3 \/environment\/guest_peer\.py "\$peer"$/mu;
const EXPERIMENTAL = /^bluetoothd --nodetach --debug --experimental /mu;

test("guest uses BusyBox network setup", () => {
  assert.match(source, VIRTIO);
  assert.match(source, CANDIDATES);
  assert.match(source, NETWORK_UP);
  assert.match(source, NETWORK_ADDRESS);
});

test("guest attaches H4 before running its control peer", () => {
  assert.match(source, BTATTACH);
  assert.match(source, EXPERIMENTAL);
  assert.match(source, PEER);
});
