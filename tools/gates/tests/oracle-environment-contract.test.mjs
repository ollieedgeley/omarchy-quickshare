import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  assertGtestEvidence,
  environmentFingerprint,
  validateEnvironment,
  validateReferenceLock,
} from "../../../tests/environments/oracle/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "oracle");
const EXPECTED_FINGERPRINT =
  "1fdde58717bb5c17b54c16c4176ab5445194181296a1eee32c795bc9be943afa";
const BASE_IMAGE_PATTERN = /^debian@sha256:/u;
const BAZEL_GTEST_ARG_PATTERN = /`--test_arg=--gtest_filter=\$\{filter\}`/u;
const BAZEL_TEST_FILTER_PATTERN = /`--test_filter=\$\{filter\}`/u;
const SHA256_MISMATCH_PATTERN = /SHA-256 mismatch/u;
const DOCKERFILE_DRIFT_PATTERN = /Dockerfile lacks manifest value/u;
const RAW_OUTPUT_ARTIFACT_PATTERN = /stdout.*recordFailureArtifact/u;
const SELECTED_COUNT_EVENT_PATTERN = /selected-\$\{selectedCount\}/u;
const GTEST_COUNT_MISMATCH_PATTERN = /expected 3 selected cases, observed 2/u;
const MISMATCHED_SELECTED_CASES = 3;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("oracle medium self-tests pass selected GTest filters to Bazel", () => {
  const source = readFileSync(join(DIRECTORY, "environment.mjs"), "utf8");
  assert.ok(source.includes("--test_output=all"));
  assert.match(source, BAZEL_GTEST_ARG_PATTERN);
  assert.doesNotMatch(source, BAZEL_TEST_FILTER_PATTERN);
  assert.ok(!source.includes("gtest_fail_if_no_test_selected"));
});

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

test("oracle BWU gates use exact simulated reference selections", () => {
  const source = readFileSync(join(DIRECTORY, "environment.mjs"), "utf8");
  assert.ok(source.includes("bwu-handler-self-test"));
  assert.ok(source.includes("bwu-fallback-self-test"));
  assert.ok(source.includes("BluetoothBwuTest.*"));
  assert.ok(source.includes("BwuManagerTest.InitiateBwu_Revert_OnDisconnect"));
  assert.ok(source.includes("Revert_OnDisconnect_WifiDirect"));
  assert.ok(source.includes("Revert_OnDisconnect_Hotspot"));
  assert.ok(source.includes("Revert_OnDisconnect_Wlan"));
  assert.ok(source.includes("Revert_OnUpgradeFailure_FlagEnabled"));
  assert.ok(source.includes("Revert_OnUpgradeFailure_FlagDisabled"));
});

test("oracle BWU failures record fixed metadata without Bazel output", () => {
  const source = readFileSync(join(DIRECTORY, "environment.mjs"), "utf8");
  assert.ok(source.includes("recordFailureArtifact"));
  assert.match(source, SELECTED_COUNT_EVENT_PATTERN);
  assert.doesNotMatch(source, RAW_OUTPUT_ARTIFACT_PATTERN);
});

test("oracle overlay replaces Google's unavailable GTest target", () => {
  const source = readFileSync(join(DIRECTORY, "google-overlay.mjs"), "utf8");
  assert.ok(source.includes('internal", "platform", "BUILD'));
  assert.ok(source.includes('connections", "implementation", "BUILD'));
  assert.ok(source.includes("@com_google_googletest//:gtest"));
});

test("oracle overlay preserves Wi-Fi LAN upgrade-frame comparison", () => {
  const source = readFileSync(join(DIRECTORY, "google-overlay.mjs"), "utf8");
  assert.ok(source.includes("EqualsProto(expected_frame)"));
  assert.ok(source.includes("result_frame.SerializeAsString"));
  assert.ok(source.includes("expected_frame.SerializeAsString"));
});

test("oracle handler test excludes its unselected AWDL source", () => {
  const source = readFileSync(join(DIRECTORY, "google-overlay.mjs"), "utf8");
  assert.ok(source.includes("awdl_bwu_handler_test.cc"));
});

test("oracle environment pins every reproducibility input", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.equal(parsed.bazel.version, "9.2.0");
  assert.match(parsed.base, BASE_IMAGE_PATTERN);
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
  assert.ok(
    parsed.reference.targets.includes(
      "//connections/implementation/mediums:bwu_handler_test",
    ),
  );
  assert.ok(
    parsed.reference.targets.includes("//connections/implementation:bwu_test"),
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
