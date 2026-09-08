import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

import {
  assertGtestEvidence,
  validateEnvironment,
  validateReferenceLock,
} from "../../../tests/environments/oracle/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "oracle");
const SHA256_MISMATCH_PATTERN = /SHA-256 mismatch/u;
const DOCKERFILE_DRIFT_PATTERN = /Dockerfile lacks manifest value/u;
const GTEST_COUNT_MISMATCH_PATTERN = /expected 3 selected cases, observed 2/u;
const MISMATCHED_SELECTED_CASES = 3;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("oracle evidence requires every selected GTest case to pass", () => {
  assert.equal(
    assertGtestEvidence(
      `
[ RUN      ] Suite.First
[       OK ] Suite.First (0 ms)
[ RUN      ] Suite.Second
[       OK ] Suite.Second (0 ms)
[  PASSED  ] 2 tests.
`,
      2,
    ),
    2,
  );
  assert.throws(() =>
    assertGtestEvidence("This program contains GoogleTest.", 1),
  );
  assert.throws(() => assertGtestEvidence("[  PASSED  ] 0 tests.", 1));
  assert.throws(() =>
    assertGtestEvidence(
      `
[ RUN      ] Suite.First
[  PASSED  ] 1 test.
`,
      1,
    ),
  );
  assert.throws(
    () =>
      assertGtestEvidence(
        `
[ RUN      ] Suite.First
[       OK ] Suite.First (0 ms)
[ RUN      ] Suite.Second
[       OK ] Suite.Second (0 ms)
[  PASSED  ] 2 tests.
`,
        MISMATCHED_SELECTED_CASES,
      ),
    GTEST_COUNT_MISMATCH_PATTERN,
  );
});

test("oracle environment accepts its recorded reference lock", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  validateReferenceLock(
    parsed,
    readFileSync(join(DIRECTORY, parsed.reference.lockFile)),
  );
});

test("oracle environment rejects a changed reference lock", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.throws(
    () => validateReferenceLock(parsed, Buffer.from("not the lock")),
    SHA256_MISMATCH_PATTERN,
  );
});

test("oracle environment rejects drift between manifest and Dockerfile", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    debianSnapshot: "20260831T000000Z",
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    DOCKERFILE_DRIFT_PATTERN,
  );
});
