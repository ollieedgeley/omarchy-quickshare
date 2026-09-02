import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
} from "../../../tests/environments/android/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const MANIFEST = join(
  ROOT,
  "tests",
  "environments",
  "android",
  "environment.json",
);
const EXPECTED_PACKAGE_COUNT = 6;
const EXPECTED_TOOL_COUNT = 2;
const SHA1_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const INVALID_SHA256_PATTERN = /invalid package SHA-256/u;
const TWO_PEERS_PATTERN = /exactly two peers/u;

function source() {
  return readFileSync(MANIFEST, "utf8");
}

test("Android environment pins every SDK and AVD input", () => {
  const manifest = validateEnvironment(source());
  assert.equal(manifest.packages.length, EXPECTED_PACKAGE_COUNT);
  assert.equal(manifest.probe.toolchain.length, EXPECTED_TOOL_COUNT);
  assert.equal(manifest.avds.peers.length, 2);
  assert.match(environmentFingerprint(source()), SHA256_PATTERN);
  for (const record of manifest.packages) {
    assert.match(record.sha1, SHA1_PATTERN);
    assert.match(record.sha256, SHA256_PATTERN);
  }
  for (const record of manifest.probe.toolchain) {
    assert.match(record.sha256, SHA256_PATTERN);
  }
});

test("Android environment rejects mutable package checksums", () => {
  const manifest = JSON.parse(source());
  manifest.packages[0].sha256 = "latest";
  assert.throws(
    () => validateEnvironment(JSON.stringify(manifest)),
    INVALID_SHA256_PATTERN,
  );
});

test("Android environment rejects a single-peer control", () => {
  const manifest = JSON.parse(source());
  manifest.avds.peers.pop();
  assert.throws(
    () => validateEnvironment(JSON.stringify(manifest)),
    TWO_PEERS_PATTERN,
  );
});
