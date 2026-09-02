import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
  validateReferenceLock,
} from "../../../tests/environments/oracle/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "oracle");
const EXPECTED_FINGERPRINT =
  "1fdde58717bb5c17b54c16c4176ab5445194181296a1eee32c795bc9be943afa";

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("oracle environment pins every reproducibility input", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.equal(parsed.bazel.version, "9.2.0");
  assert.match(parsed.base, /^debian@sha256:/u);
  assert.deepEqual(parsed.reference.sources, [
    "google-nearby",
    "google-ukey2",
    "nisaba",
    "nlohmann-json",
    "protobuf-matchers",
    "smhasher",
  ]);
  assert.ok(
    parsed.reference.targets.includes(
      "//connections/implementation/mediums:core_internal_mediums_test",
    ),
  );
  assert.equal(
    environmentFingerprint(manifest, dockerfile),
    EXPECTED_FINGERPRINT,
  );
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
    /SHA-256 mismatch/u,
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
    /Dockerfile lacks manifest value/u,
  );
});
