import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
} from "../../../tests/environments/proxies/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "proxies");

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("proxy environment pins its image, snapshot, and source", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.match(parsed.base, /^debian@sha256:/);
  assert.equal(parsed.go.version, "1.27.1");
  assert.match(parsed.go.sha256, /^[0-9a-f]{64}$/);
  assert.match(parsed.source.revision, /^[0-9a-f]{40}$/);
  assert.match(environmentFingerprint(manifest, dockerfile), /^[0-9a-f]{64}$/);
});

test("proxy environment rejects a mutable base image", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    base: "debian:trixie",
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    /SHA-256 digest/,
  );
});
