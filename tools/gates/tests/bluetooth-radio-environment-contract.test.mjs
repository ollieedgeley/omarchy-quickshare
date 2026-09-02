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
  assert.match(parsed.sources.bluez, /^[0-9a-f]{40}$/u);
  assert.match(parsed.sources.bumble, /^[0-9a-f]{40}$/u);
  assert.match(parsed.sources["typing-extensions"], /^[0-9a-f]{40}$/u);
  assert.match(
    radioEnvironmentFingerprint(manifest, dockerfile),
    /^[0-9a-f]{64}$/u,
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
    /full commit/u,
  );
});

test(GUEST_ISOLATION_TEST, () => {
  const dockerfile = source("Dockerfile.radio");
  const guest = source("radio-guest-init.sh");
  const manager = source("radio-environment.mjs");
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");

  for (const module of ["9p", "9pnet", "9pnet_virtio", "virtio_pci"]) {
    assert.match(dockerfile, new RegExp(`(?:^|\\s)${module}(?:\\s|$)`, "u"));
  }
  for (const command of ["RUN_CONTROLLER", "RUN_BLE", "RUN_CLASSIC"]) {
    assert.match(guest, new RegExp(command, "u"));
    assert.match(manager, new RegExp(command, "u"));
  }
  for (const target of [
    "test-bluetooth-controller",
    "test-bluetooth-ble",
    "test-bluetooth-classic",
  ]) {
    assert.match(makefile, new RegExp(`^${target}:`, "mu"));
  }
  assert.match(manager, /--network=none/u);
  assert.match(manager, /--device=\/dev\/kvm/u);
  assert.match(manager, /org\.omarchy-quickshare\.fingerprint/u);
});
