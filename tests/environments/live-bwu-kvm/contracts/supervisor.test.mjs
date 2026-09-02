import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../supervisor.sh", import.meta.url),
  "utf8",
);
const RELAY = /python3 \/environment\/h4_relay\.py &/u;
const TWO_PEERS = /if \[ "\$\{OQS_PEERS:-two\}" = two \]; then/u;
const SECOND = /qemu b connect=127\.0\.0\.1:45551/u;

test("one-peer mode omits the second guest", () => {
  assert.match(source, RELAY);
  assert.match(source, TWO_PEERS);
  assert.match(source, SECOND);
});
