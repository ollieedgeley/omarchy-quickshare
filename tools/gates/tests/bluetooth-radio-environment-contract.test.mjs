import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  radioEnvironmentFingerprint,
  validateRadioEnvironment,
} from "../../../tests/environments/bluez/radio-environment.mjs";

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
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");

  for (const module of ["9p", "9pnet", "9pnet_virtio", "virtio_pci"]) {
    const modulePattern = new RegExp(`(?:^|\\s)${module}(?:\\s|$)`, "u");
    assert.match(dockerfile, modulePattern);
  }
  for (const command of ["RUN_CONTROLLER", "RUN_BLE", "RUN_CLASSIC"]) {
    const commandPattern = new RegExp(command, "u");
    assert.match(guest, commandPattern);
    assert.match(manager, commandPattern);
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
});
