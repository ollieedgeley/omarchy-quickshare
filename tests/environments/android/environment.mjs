import { createHash } from "node:crypto";
import { accessSync, constants, readFileSync } from "node:fs";
import { arch, platform } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  bootstrapAndroid,
  provisionAndroid,
  reviewAndroidLicense,
} from "./provision.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const REQUIRED_PACKAGE_IDS = [
  "build-tools;36.1.0",
  "cmdline-tools;23.0",
  "emulator",
  "platform-tools",
  "platforms;android-36",
  "system-images;android-36;google_apis_playstore;x86_64",
];
const REQUIRED_REPOSITORIES = ["gradle", "sdk", "systemImages", "temurin"];
const REQUIRED_TOOL_IDS = ["gradle", "java"];
const EXPECTED_PEER_COUNT = 2;
const KVM_PATH = "/dev/kvm";
const LICENSE_IDENTIFIER = "android-sdk-license";
const LICENSE_TERMS_URL = "https://developer.android.com/studio/terms";
const GRADLE_REPOSITORY_PATTERN =
  /^https:\/\/services\.gradle\.org\/distributions\/$/u;
const SDK_REPOSITORY_PATTERN =
  /^https:\/\/dl\.google\.com\/android\/repository\/$/u;
const SYSTEM_IMAGE_REPOSITORY_PATTERN = new RegExp(
  "^https://dl\\.google\\.com/android/repository/sys-img/" +
    "google_apis_playstore/$",
  "u",
);
const TEMURIN_REPOSITORY_PATTERN = new RegExp(
  "^https://github\\.com/adoptium/temurin21-binaries/releases/" +
    "download/jdk-21\\.0\\.12\\.1%2B1/$",
  "u",
);
const REPOSITORY_PATTERNS = {
  gradle: GRADLE_REPOSITORY_PATTERN,
  sdk: SDK_REPOSITORY_PATTERN,
  systemImages: SYSTEM_IMAGE_REPOSITORY_PATTERN,
  temurin: TEMURIN_REPOSITORY_PATTERN,
};
const PACKAGE_ID_PATTERN = /^[\w.;-]+$/u;
const REVISION_PATTERN = /^\d+(?:\.\d+)*(?:\+\d+)?$/u;
const ARCHIVE_PATTERN = /^[\w.-]+\.(?:tar\.gz|zip)$/u;
const SHA1_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const PEER_NAME_PATTERN = /^quickshare-[a-z]$/u;
const SEMANTIC_VERSION_PATTERN = /^\d+\.\d+\.\d+$/u;

function assertExactKeys(actual, expected, owner) {
  const actualKeys = Object.keys(actual).sort();
  const expectedKeys = [...expected].sort();
  if (JSON.stringify(actualKeys) !== JSON.stringify(expectedKeys)) {
    throw new Error(`${owner} has unexpected or missing fields`);
  }
}

function assertMatch(value, pattern, message) {
  if (typeof value !== "string" || !pattern.test(value)) {
    throw new Error(message);
  }
}

function validateRepository(repository, name) {
  const pattern = REPOSITORY_PATTERNS[name];
  assertMatch(repository, pattern, `invalid Android ${name} repository`);
}

function validateHost(host) {
  assertExactKeys(
    host,
    ["architecture", "operatingSystem", "requiresKvm"],
    "Android host",
  );
  if (typeof host.requiresKvm !== "boolean") {
    throw new TypeError("Android host requiresKvm must be boolean");
  }
}

function validateLicense(license) {
  assertExactKeys(license, ["identifier", "termsUrl"], "Android license");
  if (
    license.identifier !== LICENSE_IDENTIFIER ||
    license.termsUrl !== LICENSE_TERMS_URL
  ) {
    throw new Error("Android SDK license metadata is unexpected");
  }
}

function validatePackage(record, repositories) {
  assertExactKeys(
    record,
    ["archive", "id", "revision", "sha1", "sha256", "size", "source"],
    `Android package ${record.id ?? "<unknown>"}`,
  );
  assertMatch(record.id, PACKAGE_ID_PATTERN, "invalid Android package id");
  assertMatch(record.revision, REVISION_PATTERN, "invalid package revision");
  assertMatch(record.archive, ARCHIVE_PATTERN, "invalid archive name");
  assertMatch(record.sha1, SHA1_PATTERN, "invalid package SHA-1");
  assertMatch(record.sha256, SHA256_PATTERN, "invalid package SHA-256");
  if (!Number.isSafeInteger(record.size) || record.size <= 0) {
    throw new Error(`Android package ${record.id} has an invalid size`);
  }
  if (!Object.hasOwn(repositories, record.source)) {
    throw new Error(`Android package ${record.id} has an unknown source`);
  }
}

function validateArtifact(record, repositories, owner) {
  assertExactKeys(
    record,
    ["archive", "id", "revision", "sha256", "size", "source"],
    owner,
  );
  assertMatch(record.archive, ARCHIVE_PATTERN, `${owner} has invalid archive`);
  assertMatch(
    record.revision,
    REVISION_PATTERN,
    `${owner} has invalid revision`,
  );
  assertMatch(record.sha256, SHA256_PATTERN, `${owner} has invalid SHA-256`);
  if (!Number.isSafeInteger(record.size) || record.size <= 0) {
    throw new Error(`${owner} has invalid size`);
  }
  if (!Object.hasOwn(repositories, record.source)) {
    throw new Error(`${owner} has an unknown source`);
  }
}

function validatePackages(packages, repositories) {
  if (!Array.isArray(packages)) {
    throw new TypeError("Android packages must be an array");
  }
  for (const record of packages) {
    validatePackage(record, repositories);
  }
  const ids = packages.map(({ id }) => id);
  if (new Set(ids).size !== ids.length) {
    throw new Error("Android package ids must be unique");
  }
  if (
    JSON.stringify([...ids].sort()) !== JSON.stringify(REQUIRED_PACKAGE_IDS)
  ) {
    throw new Error("Android package set is incomplete or unexpected");
  }
}

function validatePeer(peer) {
  assertExactKeys(peer, ["consolePort", "name"], "Android peer");
  assertMatch(peer.name, PEER_NAME_PATTERN, "invalid Android peer name");
  if (!Number.isSafeInteger(peer.consolePort) || peer.consolePort % 2 !== 0) {
    throw new Error(`Android peer ${peer.name} needs an even console port`);
  }
}

function validateAvds(avds, packages) {
  assertExactKeys(
    avds,
    ["hardwareProfile", "locale", "peers", "systemImage"],
    "Android AVD configuration",
  );
  if (!packages.some(({ id }) => id === avds.systemImage)) {
    throw new Error("Android AVD system image is not pinned");
  }
  if (avds.peers.length !== EXPECTED_PEER_COUNT) {
    throw new Error("Android environment requires exactly two peers");
  }
  for (const peer of avds.peers) {
    validatePeer(peer);
  }
  const names = avds.peers.map(({ name }) => name);
  const ports = avds.peers.map(({ consolePort }) => consolePort);
  if (
    new Set(names).size !== names.length ||
    new Set(ports).size !== ports.length
  ) {
    throw new Error("Android peer names and ports must be unique");
  }
}

function validateToolchain(toolchain, repositories) {
  if (!Array.isArray(toolchain)) {
    throw new TypeError("Android probe toolchain must be an array");
  }
  for (const record of toolchain) {
    validateArtifact(record, repositories, `Android probe tool ${record.id}`);
  }
  const ids = toolchain.map(({ id }) => id).sort();
  if (JSON.stringify(ids) !== JSON.stringify(REQUIRED_TOOL_IDS)) {
    throw new Error("Android probe toolchain is incomplete or unexpected");
  }
}

function validateProbe(probe, repositories) {
  assertExactKeys(
    probe,
    [
      "androidGradlePlugin",
      "compileSdk",
      "dependencies",
      "minSdk",
      "targetSdk",
      "toolchain",
    ],
    "Android probe",
  );
  assertMatch(
    probe.androidGradlePlugin,
    SEMANTIC_VERSION_PATTERN,
    "probe tools need exact versions",
  );
  for (const [name, value] of Object.entries(probe.dependencies)) {
    assertMatch(
      value,
      SEMANTIC_VERSION_PATTERN,
      `${name} needs an exact version`,
    );
  }
  if (probe.minSdk > probe.targetSdk || probe.targetSdk > probe.compileSdk) {
    throw new Error("Android probe SDK levels are inconsistent");
  }
  validateToolchain(probe.toolchain, repositories);
}

export function validateEnvironment(source) {
  const manifest = JSON.parse(source);
  assertExactKeys(
    manifest,
    ["avds", "host", "license", "packages", "probe", "repositories", "schema"],
    "Android environment",
  );
  if (manifest.schema !== 1) {
    throw new Error("unsupported Android environment schema");
  }
  validateHost(manifest.host);
  validateLicense(manifest.license);
  assertExactKeys(
    manifest.repositories,
    REQUIRED_REPOSITORIES,
    "Android repositories",
  );
  for (const [name, repository] of Object.entries(manifest.repositories)) {
    validateRepository(repository, name);
  }
  validatePackages(manifest.packages, manifest.repositories);
  validateAvds(manifest.avds, manifest.packages);
  validateProbe(manifest.probe, manifest.repositories);
  return manifest;
}

export function environmentFingerprint(source) {
  validateEnvironment(source);
  return createHash("sha256").update(source).digest("hex");
}

function expectedHost(manifest) {
  const architectureAliases = { x64: "x86_64" };
  const architecture = architectureAliases[arch()] ?? arch();
  return {
    architecture,
    operatingSystem: platform(),
    requiresKvm: manifest.host.requiresKvm,
  };
}

function preflight(manifest) {
  const actual = expectedHost(manifest);
  if (actual.architecture !== manifest.host.architecture) {
    throw new Error(
      `Android environment does not support ${actual.architecture}`,
    );
  }
  if (actual.operatingSystem !== manifest.host.operatingSystem) {
    throw new Error(
      `Android environment does not support ${actual.operatingSystem}`,
    );
  }
  if (actual.requiresKvm) {
    accessSync(KVM_PATH, constants.R_OK);
    accessSync(KVM_PATH, constants.W_OK);
  }
  process.stdout.write("Android host preflight passed with usable KVM.\n");
}

function loadManifest() {
  return validateEnvironment(readFileSync(MANIFEST_PATH, "utf8"));
}

async function main() {
  const [, , command] = process.argv;
  if (command === "validate") {
    loadManifest();
    return;
  }
  if (command === "preflight") {
    preflight(loadManifest());
    return;
  }
  if (command === "bootstrap") {
    preflight(loadManifest());
    await bootstrapAndroid(loadManifest());
    return;
  }
  if (command === "license") {
    preflight(loadManifest());
    await reviewAndroidLicense(loadManifest());
    return;
  }
  if (command === "provision") {
    preflight(loadManifest());
    await provisionAndroid(loadManifest());
    return;
  }
  throw new Error(
    "usage: environment.mjs <validate|preflight|bootstrap|license|provision>",
  );
}

const [, invokedArgument = ""] = process.argv;
const invokedPath = resolve(invokedArgument);
if (invokedPath === fileURLToPath(import.meta.url)) {
  await main();
}
