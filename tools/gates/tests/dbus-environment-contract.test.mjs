import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

const { validateEnvironment } =
  await import("../../../tests/environments/bluez/dbus-environment.mjs");

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "bluez");
const FULL_COMMIT_PATTERN = /full commit/u;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "dbus-environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.dbus"), "utf8"),
  };
}

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
