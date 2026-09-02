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
  assert.match(environmentFingerprint(manifest, dockerfile), /^[0-9a-f]{64}$/u);
});

test("D-Bus environment rejects a shortened source revision", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    source: { id: "python-dbusmock", revision: "45885bf" },
  });
  assert.throws(() => validateEnvironment(changed, dockerfile), /full commit/u);
});
