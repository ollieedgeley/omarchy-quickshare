import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  radioEnvironmentFingerprint,
  validateRadioEnvironment,
} from "../../../tests/environments/bluez/radio-environment.mjs";
const { btsnoopTraceMetadata, writeBtsnoopTraceSummary } = await import(
  new URL("../../../tests/environments/bluez/radio-test.mjs", import.meta.url)
);

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "bluez");
const GUEST_ISOLATION_TEST =
  "Bluetooth radio guest isolates real BlueZ and both transport proofs";
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const FULL_COMMIT_PATTERN = /full commit/u;
const NO_NETWORK_PATTERN = /--network=none/u;
const KVM_DEVICE_PATTERN = /--device=\/dev\/kvm/u;
const FINGERPRINT_LABEL_PATTERN = /org\.omarchy-quickshare\.fingerprint/u;
const BTSNOOP_METADATA_TEST = [
  "Bluetooth radio failure trace retains only ",
  "btsnoop record metadata",
].join("");
const BTSNOOP_HEADER_BYTES = 16;
const BTSNOOP_INCLUDED_LENGTH_OFFSET = 4;
const BTSNOOP_VERSION_OFFSET = 8;
const BTSNOOP_DATALINK_OFFSET = 12;
const BTSNOOP_RECORD_BYTES = 28;
const BTSNOOP_PACKET_BYTES = 4;
const BTSNOOP_UART_H4_DATALINK = 1002;
const BTSNOOP_HEADER_ERROR_PATTERN = /btsnoop header/u;
const TRUNCATED_BTSNOOP_ERROR_PATTERN = /truncated btsnoop/u;
const PRIVATE_TRACE_VALUE_PATTERN = /private-payload/u;
const H4_COMMAND_PACKET = 1;
const COMMAND_OPCODE = 0x1234;
const COMMAND_OPCODE_LOW = 0x34;
const COMMAND_OPCODE_HIGH = 0x12;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "radio-environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.radio"), "utf8"),
  };
}

function source(name) {
  return readFileSync(join(DIRECTORY, name), "utf8");
}

test("Bluetooth radio environment pins BlueZ and Bumble", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateRadioEnvironment(manifest, dockerfile);
  assert.match(parsed.sources.bluez, REVISION_PATTERN);
  assert.match(parsed.sources.bumble, REVISION_PATTERN);
  assert.match(parsed.sources["typing-extensions"], REVISION_PATTERN);
  assert.match(
    radioEnvironmentFingerprint(manifest, dockerfile),
    SHA256_PATTERN,
  );
});

test("Bluetooth radio environment rejects mutable source revisions", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    sources: { ...JSON.parse(manifest).sources, bumble: "main" },
  });
  assert.throws(
    () => validateRadioEnvironment(changed, dockerfile),
    FULL_COMMIT_PATTERN,
  );
});

test(GUEST_ISOLATION_TEST, () => {
  const dockerfile = source("Dockerfile.radio");
  const guest = source("radio-guest-init.sh");
  const manager = source("radio-environment.mjs");
  const selfTest = source("radio-test.mjs");
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");

  for (const module of ["9p", "9pnet", "9pnet_virtio", "virtio_pci"]) {
    const modulePattern = new RegExp(`(?:^|\\s)${module}(?:\\s|$)`, "u");
    assert.match(dockerfile, modulePattern);
  }
  for (const command of ["RUN_CONTROLLER", "RUN_BLE", "RUN_CLASSIC"]) {
    const commandPattern = new RegExp(command, "u");
    assert.match(guest, commandPattern);
    assert.match(selfTest, commandPattern);
  }
  for (const target of [
    "test-bluetooth-controller",
    "test-bluetooth-ble",
    "test-bluetooth-classic",
  ]) {
    const targetPattern = new RegExp(`^${target}:`, "mu");
    assert.match(makefile, targetPattern);
  }
  assert.match(manager, NO_NETWORK_PATTERN);
  assert.match(manager, KVM_DEVICE_PATTERN);
  assert.match(manager, FINGERPRINT_LABEL_PATTERN);
  assert.equal(manager.includes("copyFileSync(TRACE"), false);
});

test(BTSNOOP_METADATA_TEST, () => {
  const directory = mkdtempSync("/tmp/quickshare-btsnoop-");
  const trace = join(directory, "radio.btsnoop");
  try {
    const header = Buffer.alloc(BTSNOOP_HEADER_BYTES);
    header.write("btsnoop\0");
    header.writeUInt32BE(1, BTSNOOP_VERSION_OFFSET);
    header.writeUInt32BE(BTSNOOP_UART_H4_DATALINK, BTSNOOP_DATALINK_OFFSET);
    const record = Buffer.alloc(BTSNOOP_RECORD_BYTES);
    record.writeUInt32BE(BTSNOOP_PACKET_BYTES, 0);
    record.writeUInt32BE(BTSNOOP_PACKET_BYTES, BTSNOOP_INCLUDED_LENGTH_OFFSET);
    writeFileSync(trace, Buffer.concat([header, record]));
    assert.deepEqual(btsnoopTraceMetadata(trace), {
      bytes: BTSNOOP_PACKET_BYTES,
      format: "btsnoop",
      records: 1,
    });
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

test("Bluetooth failure traces retain sanitized packet evidence", () => {
  const directory = mkdtempSync("/tmp/quickshare-btsnoop-");
  const trace = join(directory, "radio.btsnoop");
  const summary = join(directory, "radio-trace.json");
  try {
    const header = Buffer.alloc(BTSNOOP_HEADER_BYTES);
    header.write("btsnoop\0");
    header.writeUInt32BE(1, BTSNOOP_VERSION_OFFSET);
    header.writeUInt32BE(BTSNOOP_UART_H4_DATALINK, BTSNOOP_DATALINK_OFFSET);
    const payload = Buffer.concat([
      Buffer.from([H4_COMMAND_PACKET, COMMAND_OPCODE_LOW, COMMAND_OPCODE_HIGH]),
      Buffer.from("private-payload"),
    ]);
    const record = Buffer.alloc(
      BTSNOOP_RECORD_BYTES + payload.length - BTSNOOP_PACKET_BYTES,
    );
    record.writeUInt32BE(payload.length, 0);
    record.writeUInt32BE(payload.length, BTSNOOP_INCLUDED_LENGTH_OFFSET);
    payload.copy(record, BTSNOOP_RECORD_BYTES - BTSNOOP_PACKET_BYTES);
    writeFileSync(trace, Buffer.concat([header, record]));
    writeBtsnoopTraceSummary(trace, summary);
    const evidence = readFileSync(summary, "utf8");
    assert.doesNotMatch(evidence, PRIVATE_TRACE_VALUE_PATTERN);
    assert.equal(JSON.parse(evidence).packets[0].commandOpcode, COMMAND_OPCODE);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

test("Bluetooth radio rejects malformed btsnoop metadata", () => {
  const directory = mkdtempSync("/tmp/quickshare-btsnoop-");
  const trace = join(directory, "radio.btsnoop");
  try {
    writeFileSync(trace, Buffer.alloc(BTSNOOP_HEADER_BYTES));
    assert.throws(
      () => btsnoopTraceMetadata(trace),
      BTSNOOP_HEADER_ERROR_PATTERN,
    );
    const header = Buffer.alloc(BTSNOOP_HEADER_BYTES);
    header.write("btsnoop\0");
    header.writeUInt32BE(1, BTSNOOP_VERSION_OFFSET);
    header.writeUInt32BE(BTSNOOP_UART_H4_DATALINK, BTSNOOP_DATALINK_OFFSET);
    const record = Buffer.alloc(BTSNOOP_RECORD_BYTES);
    record.writeUInt32BE(
      BTSNOOP_PACKET_BYTES + 1,
      BTSNOOP_INCLUDED_LENGTH_OFFSET,
    );
    writeFileSync(trace, Buffer.concat([header, record]));
    assert.throws(
      () => btsnoopTraceMetadata(trace),
      TRUNCATED_BTSNOOP_ERROR_PATTERN,
    );
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});
