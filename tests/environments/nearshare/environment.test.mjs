import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { environmentFingerprint, validateEnvironment } from "./environment.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const SHA256_ERROR_PATTERN = /SHA-256 digest/u;

function inputs() {
  return {
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
  };
}

test("NearShare peer pins its source, image, and package set", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.match(parsed.source.revision, REVISION_PATTERN);
  assert.match(environmentFingerprint(manifest, dockerfile), SHA256_PATTERN);
  assert.ok(Object.keys(parsed.packages).length > 0);
});

test("NearShare peer rejects a mutable base image", () => {
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
