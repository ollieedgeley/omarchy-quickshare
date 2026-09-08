import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

import {
  coldEmulatorArguments,
  emulatorArguments,
  orchestratorRunArguments,
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
import {
  androidEnvironment,
  commandPaths,
  environmentPaths,
} from "../../../tests/environments/android/paths.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const MANIFEST = join(
  ROOT,
  "tests",
  "environments",
  "android",
  "environment.json",
);
const FIXTURE_USER_ID = 1000;
const PRIVILEGED_PORT = 22;
const SHA256_PATTERN_LENGTH = 64;
const INVALID_SHA256_PATTERN = /invalid package SHA-256/u;
const TWO_PEERS_PATTERN = /exactly two peers/u;
const INVALID_ADB_PORT_PATTERN = /adbServerPort must be a user port/u;
const FIXTURE_BYTES = "pinned Android archive fixture";

function source() {
  return readFileSync(MANIFEST, "utf8");
}

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

test("Android environment rejects a privileged ADB server port", () => {
  const manifest = JSON.parse(source());
  manifest.host.adbServerPort = PRIVILEGED_PORT;
  assert.throws(
    () => validateEnvironment(JSON.stringify(manifest)),
    INVALID_ADB_PORT_PATTERN,
  );
});

test("Android package pins map to exact CLI install arguments", () => {
  const records = new Map([
    ["platform-tools", { id: "platform-tools", revision: "1.2.3" }],
    ["platforms;android-31", { id: "platforms;android-31", revision: "4" }],
    [
      "system-images;android-31;google_apis;x86_64",
      {
        id: "system-images;android-31;google_apis;x86_64",
        revision: "5",
      },
    ],
  ]);
  assert.equal(
    sdkPackageArgument(records.get("platform-tools")),
    "platform-tools@1.2.3",
  );
  assert.equal(
    sdkPackageArgument(records.get("platforms;android-31")),
    "platforms/android-31@4",
  );
  assert.equal(
    sdkPackageArgument(
      records.get("system-images;android-31;google_apis;x86_64"),
    ),
    "system-images/android-31/google_apis/x86_64@5",
  );
  assert.deepEqual(
    sdkInstallArguments({ sdk: "/sdk" }, [records.get("platform-tools")]),
    ["--sdk=/sdk", "sdk", "install", "platform-tools@1.2.3"],
  );
});

test("Android tools share one isolated ADB identity", () => {
  const manifest = validateEnvironment(source());
  const paths = environmentPaths("/cache-root");
  const commands = commandPaths(paths, manifest);
  const environment = androidEnvironment(paths, commands, manifest);
  assert.ok(environment.HOME.startsWith("/cache-root/"));
  assert.ok(
    environment.ANDROID_EMULATOR_HOME.startsWith(`${environment.HOME}/`),
  );
  assert.equal(
    environment.ANDROID_ADB_SERVER_PORT,
    String(manifest.host.adbServerPort),
  );
  assert.equal(
    environment.ADB_VENDOR_KEYS,
    join(environment.ANDROID_EMULATOR_HOME, "adbkey"),
  );
});

test("Android archives resolve against their configured repository", () => {
  const manifest = {
    repositories: { gradle: "https://example.invalid/distributions/" },
  };
  const gradle = { source: "gradle", archive: "fixture.zip" };
  assert.equal(
    archiveUrl(manifest, gradle),
    "https://example.invalid/distributions/fixture.zip",
  );
  assert.equal(
    archivePath({ archives: "/cache" }, gradle),
    "/cache/fixture.zip",
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

test("Android downloads preserve bytes and reject a wrong hash", async () => {
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

test("Android emulator arguments request headless and cold-start modes", () => {
  const manifest = validateEnvironment(source());
  const commands = { adb: "/sdk/platform-tools/adb" };
  const argumentsList = emulatorArguments(
    manifest.avds.peers[0],
    commands,
    manifest.avds,
  );
  const acceleration = argumentsList.indexOf("-accel");
  const adbPath = argumentsList.indexOf("-adb-path");
  const memory = argumentsList.indexOf("-memory");
  assert.ok(argumentsList.includes("-no-window"));
  assert.ok(!argumentsList.includes("-no-snapshot"));
  assert.equal(argumentsList[acceleration + 1], "on");
  assert.equal(argumentsList[adbPath + 1], commands.adb);
  assert.equal(
    argumentsList[memory + 1],
    String(manifest.avds.memoryMegabytes),
  );
  const coldArguments = coldEmulatorArguments(
    manifest.avds.peers[0],
    commands,
    manifest.avds,
  );
  assert.ok(coldArguments.includes("-no-snapshot-load"));
  assert.ok(coldArguments.includes("-wipe-data"));
});

test("Mobly arguments restrict privileges and select pinned tools", () => {
  const manifest = validateEnvironment(source());
  const argumentsList = orchestratorRunArguments(
    manifest,
    { diagnostics: "/diagnostics", sdk: "/sdk" },
    { gid: FIXTURE_USER_ID, uid: FIXTURE_USER_ID },
  );
  assert.ok(argumentsList.includes("--read-only"));
  assert.ok(argumentsList.includes("--cap-drop=ALL"));
  assert.ok(
    argumentsList.includes(
      `ANDROID_ADB_SERVER_PORT=${manifest.host.adbServerPort}`,
    ),
  );
  assert.ok(
    argumentsList.includes("/sdk/platform-tools:/android-platform-tools:ro"),
  );
  assert.ok(argumentsList.includes(manifest.probe.orchestrator.image));
});
