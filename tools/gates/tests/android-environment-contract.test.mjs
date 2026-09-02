import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  coldEmulatorArguments,
  emulatorArguments,
  environmentFingerprint,
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
  orchestratorFingerprint,
  validateOrchestratorFiles,
} from "../../../tests/environments/android/orchestrator.mjs";
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
const PROBE_ROOT = join(ROOT, "tests", "environments", "android", "probe");
const PROBE_BUILD = join(PROBE_ROOT, "app", "build.gradle.kts");
const PROBE_MANIFEST = join(
  PROBE_ROOT,
  "app",
  "src",
  "main",
  "AndroidManifest.xml",
);
const PROBE_LOCK = join(PROBE_ROOT, "app", "gradle.lockfile");
const PROBE_VERIFICATION = join(
  PROBE_ROOT,
  "gradle",
  "verification-metadata.xml",
);
const EXPECTED_PACKAGE_COUNT = 6;
const EXPECTED_TOOL_COUNT = 2;
const FIXTURE_USER_ID = 1000;
const PRIVILEGED_PORT = 22;
const SHA256_PATTERN_LENGTH = 64;
const SHA1_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const INVALID_SHA256_PATTERN = /invalid package SHA-256/u;
const TWO_PEERS_PATTERN = /exactly two peers/u;
const INVALID_ADB_PORT_PATTERN = /adbServerPort must be a user port/u;
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

test("Android environment rejects a privileged ADB server port", () => {
  const manifest = JSON.parse(source());
  manifest.host.adbServerPort = PRIVILEGED_PORT;
  assert.throws(
    () => validateEnvironment(JSON.stringify(manifest)),
    INVALID_ADB_PORT_PATTERN,
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
      records.get("system-images;android-36;google_apis;x86_64"),
    ),
    "system-images/android-36/google_apis/x86_64@7",
  );
  assert.deepEqual(
    sdkInstallArguments({ sdk: "/sdk" }, [records.get("platform-tools")]),
    ["--sdk=/sdk", "sdk", "install", "platform-tools@37.0.1"],
  );
});

test("legacy AVD manager runs from the installed SDK layout", () => {
  const manifest = validateEnvironment(source());
  const commands = commandPaths({ sdk: "/sdk", tools: "/tools" }, manifest);
  assert.equal(commands.avdmanager, "/sdk/cmdline-tools/23.0/bin/avdmanager");
});

test("Android tools share one isolated ADB identity", () => {
  const manifest = validateEnvironment(source());
  const paths = environmentPaths("/cache-root");
  const commands = commandPaths(paths, manifest);
  const environment = androidEnvironment(paths, commands, manifest);
  assert.equal(environment.HOME, "/cache-root/android/adb-home");
  assert.equal(
    environment.ANDROID_EMULATOR_HOME,
    "/cache-root/android/adb-home/.android",
  );
  assert.equal(environment.ANDROID_ADB_SERVER_PORT, "5038");
  assert.equal(
    environment.ADB_VENDOR_KEYS,
    "/cache-root/android/adb-home/.android/adbkey",
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

test("Android probe build uses every pinned dependency", () => {
  const manifest = validateEnvironment(source());
  const rootBuild = readFileSync(join(PROBE_ROOT, "build.gradle.kts"), "utf8");
  const appBuild = readFileSync(PROBE_BUILD, "utf8");
  const buildTools = manifest.packages.find(({ id }) =>
    id.startsWith("build-tools;"),
  );
  assert.ok(
    rootBuild.includes(`version "${manifest.probe.androidGradlePlugin}"`),
  );
  assert.ok(appBuild.includes(`buildToolsVersion = "${buildTools.revision}"`));
  assert.ok(appBuild.includes(`compileSdk = ${manifest.probe.compileSdk}`));
  assert.ok(appBuild.includes(`minSdk = ${manifest.probe.minSdk}`));
  assert.ok(appBuild.includes(`targetSdk = ${manifest.probe.targetSdk}`));
  for (const [dependency, version] of Object.entries(
    manifest.probe.dependencies,
  )) {
    assert.ok(appBuild.includes(`${dependency}:${version}`));
  }
});

test("Android probe declares every dangerous connection permission", () => {
  const sourceText = readFileSync(PROBE_MANIFEST, "utf8");
  const permissions = [
    "android.permission.BLUETOOTH_ADVERTISE",
    "android.permission.BLUETOOTH_CONNECT",
    "android.permission.BLUETOOTH_SCAN",
    "android.permission.NEARBY_WIFI_DEVICES",
  ];
  for (const permission of permissions) {
    assert.ok(sourceText.includes(permission));
  }
});

test("Android probe locks versions and verifies dependency artifacts", () => {
  const manifest = validateEnvironment(source());
  const lock = readFileSync(PROBE_LOCK, "utf8");
  const verification = readFileSync(PROBE_VERIFICATION, "utf8");
  for (const [dependency, version] of Object.entries(
    manifest.probe.dependencies,
  )) {
    const [group, name] = dependency.split(":");
    assert.ok(lock.includes(`${group}:${name}:${version}`));
    assert.ok(verification.includes(`group="${group}" name="${name}"`));
  }
  assert.ok(verification.includes("<verify-metadata>true</verify-metadata>"));
  assert.ok(verification.includes("<sha256 value="));
});

test("Android orchestration image pins every Python artifact", () => {
  const manifest = validateEnvironment(source());
  const sources = validateOrchestratorFiles(manifest);
  assert.match(orchestratorFingerprint(manifest, sources), SHA256_PATTERN);
  assert.ok(sources.dockerfile.includes(manifest.probe.orchestrator.python));
  assert.ok(sources.dockerfile.includes("mobly-ps"));
  assert.ok(sources.processShim.includes("/proc/[0-9]*/status"));
});

test("Android peers use accelerated display-free emulator arguments", () => {
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

test("Mobly runner is isolated and uses the pinned Android tools", () => {
  const manifest = validateEnvironment(source());
  const argumentsList = orchestratorRunArguments(
    manifest,
    { diagnostics: "/diagnostics", sdk: "/sdk" },
    { gid: FIXTURE_USER_ID, uid: FIXTURE_USER_ID },
  );
  assert.ok(argumentsList.includes("--network=host"));
  assert.ok(argumentsList.includes("--read-only"));
  assert.ok(argumentsList.includes("--cap-drop=ALL"));
  assert.ok(argumentsList.includes("ANDROID_ADB_SERVER_PORT=5038"));
  assert.ok(
    argumentsList.includes("/sdk/platform-tools:/android-platform-tools:ro"),
  );
  assert.ok(argumentsList.includes(manifest.probe.orchestrator.image));
});
