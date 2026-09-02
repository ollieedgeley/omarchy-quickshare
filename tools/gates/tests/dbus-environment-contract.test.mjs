import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
} from "../../../tests/environments/bluez/dbus-environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "bluez");
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const FULL_COMMIT_PATTERN = /full commit/u;
const ARTIFACT_WRITER_PATTERN = /recordFailureArtifact/u;
const PRIVATE_DBUS_GATE_PATTERN = /private-dbus/u;
const SERVICE_SELF_TEST_STAGE_PATTERN = /service-self-test/u;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "dbus-environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.dbus"), "utf8"),
  };
}

test("D-Bus environment pins its source and real service clients", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.deepEqual(parsed.clients, ["bluetoothctl", "nmcli"]);
  assert.deepEqual(parsed.versions, {
    bluetoothctl: "5.66",
    nmcli: "1.42.4",
    python: "3.11.2",
  });
  assert.deepEqual(parsed.templates, ["bluez5", "networkmanager"]);
  assert.match(environmentFingerprint(manifest, dockerfile), SHA256_PATTERN);
});

test("D-Bus environment rejects a shortened source revision", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    source: { id: "python-dbusmock", revision: "45885bf" },
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    FULL_COMMIT_PATTERN,
  );
});

test("D-Bus failures retain only typed artifact metadata", () => {
  const runner = readFileSync(join(DIRECTORY, "dbus-environment.mjs"), "utf8");
  assert.match(runner, ARTIFACT_WRITER_PATTERN);
  assert.match(runner, PRIVATE_DBUS_GATE_PATTERN);
  assert.match(runner, SERVICE_SELF_TEST_STAGE_PATTERN);
});
