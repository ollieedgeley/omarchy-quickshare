import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
} from "../../../tests/environments/network/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "network");

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("network environment pins radio tools and enough virtual radios", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.equal(parsed.kernelModule, "mac80211_hwsim");
  assert.equal(parsed.radios, 3);
  assert.match(parsed.source.revision, /^[0-9a-f]{40}$/);
  assert.match(environmentFingerprint(manifest, dockerfile), /^[0-9a-f]{64}$/);
});

test("network environment rejects a mutable base", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    base: "debian:bookworm",
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    /SHA-256 digest/,
  );
});
