import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

import { validateEnvironment } from "./environment.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const SHA256_ERROR_PATTERN = /SHA-256 digest/u;

function inputs() {
  return {
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
  };
}

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
