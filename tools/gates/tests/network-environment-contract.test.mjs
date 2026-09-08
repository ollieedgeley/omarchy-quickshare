import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

const { validateEnvironment } =
  await import("../../../tests/environments/network/environment.mjs");

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "network");
const SHA256_ERROR_PATTERN = /SHA-256 digest/u;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

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
