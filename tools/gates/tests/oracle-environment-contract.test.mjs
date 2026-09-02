import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
} from "../../../tests/environments/oracle/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "oracle");

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("oracle environment pins every reproducibility input", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.equal(parsed.bazel.version, "9.0.1");
  assert.match(parsed.base, /^debian@sha256:/);
  assert.match(environmentFingerprint(manifest, dockerfile), /^[0-9a-f]{64}$/);
});

test("oracle environment rejects drift between manifest and Dockerfile", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    debianSnapshot: "20260831T000000Z",
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    /Dockerfile lacks manifest value/,
  );
});
