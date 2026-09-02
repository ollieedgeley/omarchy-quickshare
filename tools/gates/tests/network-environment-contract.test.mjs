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
const EXPECTED_RADIO_COUNT = 3;
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const SHA256_ERROR_PATTERN = /SHA-256 digest/u;
const ARTIFACT_WRITER_PATTERN = /recordFailureArtifact/u;
const VIRTUAL_NETWORK_GATE_PATTERN = /virtual-network/u;
const NETWORK_SELF_TEST_STAGE_PATTERN = /network-self-test/u;

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
  assert.equal(parsed.radios, EXPECTED_RADIO_COUNT);
  assert.match(parsed.source.revision, REVISION_PATTERN);
  assert.match(environmentFingerprint(manifest, dockerfile), SHA256_PATTERN);
});

test("network environment rejects a mutable base", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    base: "debian:bookworm",
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    SHA256_ERROR_PATTERN,
  );
});

test("network failures retain only typed artifact metadata", () => {
  const runner = readFileSync(join(DIRECTORY, "environment.mjs"), "utf8");
  assert.match(runner, ARTIFACT_WRITER_PATTERN);
  assert.match(runner, VIRTUAL_NETWORK_GATE_PATTERN);
  assert.match(runner, NETWORK_SELF_TEST_STAGE_PATTERN);
});
