import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  validateEnvironment,
} from "../../../tests/environments/android/environment.mjs";
import {
  archivePath,
  archiveUrl,
  fetchArchive,
} from "../../../tests/environments/android/archives.mjs";
import {
  sdkInstallArguments,
  sdkPackageArgument,
} from "../../../tests/environments/android/provision.mjs";

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
const SHA256_PATTERN_LENGTH = 64;
const SHA1_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const INVALID_SHA256_PATTERN = /invalid package SHA-256/u;
const TWO_PEERS_PATTERN = /exactly two peers/u;
const FIXTURE_BYTES = "pinned Android archive fixture";

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

test("Android package pins map to exact CLI install arguments", () => {
  const manifest = validateEnvironment(source());
  const records = new Map(
    manifest.packages.map((record) => [record.id, record]),
  );
  assert.equal(
    sdkPackageArgument(records.get("platform-tools")),
    "platform-tools@37.0.1",
  );
  assert.equal(
    sdkPackageArgument(records.get("platforms;android-36")),
    "platforms/android-36@2",
  );
  assert.equal(
    sdkPackageArgument(
      records.get("system-images;android-36;google_apis_playstore;x86_64"),
    ),
    "system-images/android-36/google_apis_playstore/x86_64@7",
  );
  assert.deepEqual(
    sdkInstallArguments({ sdk: "/sdk" }, [records.get("platform-tools")]),
    ["--sdk=/sdk", "sdk", "install", "platform-tools@37.0.1"],
  );
});

test("Android archives resolve only from pinned repositories", () => {
  const manifest = validateEnvironment(source());
  const gradle = manifest.probe.toolchain.find(({ id }) => id === "gradle");
  assert.equal(
    archiveUrl(manifest, gradle),
    "https://services.gradle.org/distributions/gradle-9.1.0-bin.zip",
  );
  assert.equal(
    archivePath({ archives: "/cache" }, gradle),
    "/cache/gradle-9.1.0-bin.zip",
  );
});

function archiveFixture(sha256) {
  return {
    archive: "fixture.zip",
    id: "fixture",
    sha256,
    size: Buffer.byteLength(FIXTURE_BYTES),
    source: "sdk",
  };
}

test("Android archive downloads are hash and size verified", async () => {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-android-"));
  const manifest = { repositories: { sdk: "https://example.invalid/" } };
  const sha256 = createHash("sha256").update(FIXTURE_BYTES).digest("hex");
  const record = archiveFixture(sha256);
  const fetchFunction = () => Promise.resolve(new Response(FIXTURE_BYTES));
  try {
    const path = await fetchArchive({
      fetchFunction,
      manifest,
      paths: { archives: directory },
      record,
    });
    assert.equal(readFileSync(path, "utf8"), FIXTURE_BYTES);
    await assert.rejects(
      fetchArchive({
        fetchFunction,
        manifest,
        paths: { archives: join(directory, "bad") },
        record: archiveFixture("0".repeat(SHA256_PATTERN_LENGTH)),
      }),
    );
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});
