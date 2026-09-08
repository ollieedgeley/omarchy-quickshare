import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { afterEach, test } from "vitest";
import { fileURLToPath } from "node:url";

import {
  createPluginRepository,
  loadReleaseArtifacts,
} from "../plugin-export.mjs";
import { HARNESS_STUBS, headlessEnvironment } from "./plugin-harness-stubs.mjs";
import { runSocketHarness } from "./socket-fixture.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const PANEL_HARNESS = join(
  ROOT,
  "tools",
  "release",
  "tests",
  "panel-harness.qml",
);
const PLUGIN_SOURCE = join(ROOT, "packaging", "omarchy-plugin");
const COMMIT_LENGTH = 40;
const HARNESS_TIMEOUT_MS = 8_000;
const SOCKET_TIMEOUT_MS = 30_000;
const QUICKSHELL = process.env.QUICKSHELL ?? "quickshell";
const SUCCESS_PATTERN = /HARNESS_OK/u;
const SOURCE_COMMIT = "a".repeat(COMMIT_LENGTH);
const SHA256_LENGTH = 64;
const CONTROL_PROTOCOL = 7;
const REJECTED_PROTOCOL = 6;
const NATIVE_COMPONENTS = {
  cli: { file: "omarchy-quickshare", sha256: "b".repeat(SHA256_LENGTH) },
  daemon: {
    file: "omarchy-quickshare-daemon",
    sha256: "d".repeat(SHA256_LENGTH),
  },
};
const PANEL_FILES = [
  "AttachmentBadge.qml",
  "ConsentView.qml",
  "IdleView.qml",
  "PeerChoiceView.qml",
  "SharePanel.qml",
  "TerminalView.qml",
  "TransferView.qml",
];
const NATIVE_COMMIT_MISMATCH =
  /native artifact sourceCommit does not match export/u;
const SOURCE_COMMIT_MISMATCH =
  /source artifact sourceCommit does not match export/u;
const temporaryDirectories = new Set();

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-plugin-"));
  temporaryDirectories.add(directory);
  return directory;
}

function writeHarnessStubs(directory) {
  for (const [file, source] of Object.entries(HARNESS_STUBS)) {
    const path = join(directory, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, source);
  }
}

function preparePanelHarness(root) {
  const directory = join(root, "harness");
  mkdirSync(directory);
  for (const file of PANEL_FILES) {
    copyFileSync(join(PLUGIN_SOURCE, file), join(directory, file));
  }
  writeHarnessStubs(directory);
  const harness = join(directory, "panel-harness.qml");
  copyFileSync(PANEL_HARNESS, harness);
  return harness;
}

afterEach(() => {
  for (const directory of temporaryDirectories) {
    rmSync(directory, { force: true, recursive: true });
  }
  temporaryDirectories.clear();
});

test("plugin export ships entry points but not private files", () => {
  const root = temporaryDirectory();
  const destination = join(root, "export");
  const source = join(root, "plugin");
  cpSync(PLUGIN_SOURCE, source, { recursive: true });
  writeFileSync(join(source, ".env"), "private credentials\n");
  mkdirSync(join(source, "tests"));
  writeFileSync(join(source, "tests", "private.txt"), "test-only data\n");

  createPluginRepository({ destination, source, sourceCommit: SOURCE_COMMIT });

  const files = readdirSync(destination)
    .filter((file) => file !== ".git")
    .sort();
  const manifest = JSON.parse(
    readFileSync(join(destination, "manifest.json"), "utf8"),
  );
  for (const file of [
    "LICENSE",
    "README.md",
    ...Object.values(manifest.entryPoints),
  ]) {
    assert.equal(lstatSync(join(destination, file)).isFile(), true);
  }
  assert.equal(existsSync(join(destination, ".env")), false);
  assert.equal(existsSync(join(destination, "tests")), false);
  assert.equal(lstatSync(join(destination, ".git")).isDirectory(), true);
  for (const file of files) {
    assert.equal(lstatSync(join(destination, file)).isSymbolicLink(), false);
  }
  const release = JSON.parse(
    readFileSync(join(destination, "release.json"), "utf8"),
  );
  assert.equal(release.sourceCommit, SOURCE_COMMIT);
  assert.deepEqual(release.controlProtocol, {
    minimum: CONTROL_PROTOCOL,
    maximum: CONTROL_PROTOCOL,
  });
  assert.equal(release.nativeArtifact.published, false);
  assert.equal("sha256" in release.nativeArtifact, false);
  assert.equal("sourceBuild" in release, false);
});

test("plugin export omits checksums when native artifacts are absent", () => {
  const source = join(temporaryDirectory(), "plugin");
  cpSync(PLUGIN_SOURCE, source, { recursive: true });
  const polluted = JSON.parse(
    readFileSync(join(source, "release.json"), "utf8"),
  );
  polluted.nativeArtifact.components = NATIVE_COMPONENTS;
  polluted.nativeArtifact.published = true;
  polluted.controlProtocol = { minimum: 1, maximum: REJECTED_PROTOCOL };
  polluted.sourceBuild = { sha256: "e".repeat(SHA256_LENGTH) };
  writeFileSync(
    join(source, "release.json"),
    `${JSON.stringify(polluted, null, 2)}\n`,
  );
  const destination = join(temporaryDirectory(), "export");

  createPluginRepository({
    destination,
    source,
    sourceCommit: SOURCE_COMMIT,
  });

  const release = JSON.parse(
    readFileSync(join(destination, "release.json"), "utf8"),
  );
  assert.equal(release.sourceCommit, SOURCE_COMMIT);
  assert.equal(release.nativeArtifact.published, false);
  assert.equal("components" in release.nativeArtifact, false);
  assert.deepEqual(release.controlProtocol, {
    minimum: CONTROL_PROTOCOL,
    maximum: CONTROL_PROTOCOL,
  });
  assert.equal("sourceBuild" in release, false);
});

test("plugin export records native and source-build checksums", () => {
  const destination = join(temporaryDirectory(), "export");
  const sourceSha256 = "c".repeat(SHA256_LENGTH);

  createPluginRepository({
    artifacts: {
      nativeComponents: NATIVE_COMPONENTS,
      nativeVersion: "0.0.0",
      sourceSha256,
    },
    destination,
    sourceCommit: SOURCE_COMMIT,
  });

  const release = JSON.parse(
    readFileSync(join(destination, "release.json"), "utf8"),
  );
  assert.equal(release.sourceCommit, SOURCE_COMMIT);
  assert.equal(release.nativeArtifact.version, "0.0.0");
  assert.deepEqual(release.nativeArtifact.components, NATIVE_COMPONENTS);
  assert.equal(release.nativeArtifact.published, true);
  assert.deepEqual(release.controlProtocol, {
    minimum: CONTROL_PROTOCOL,
    maximum: CONTROL_PROTOCOL,
  });
  assert.equal(release.sourceBuild.sha256, sourceSha256);
});

test(
  "offscreen service handles Unix socket frames and recovery",
  async () => {
    await runSocketHarness({
      root: temporaryDirectory(),
      pluginSource: PLUGIN_SOURCE,
      quickshell: QUICKSHELL,
    });
  },
  SOCKET_TIMEOUT_MS,
);

test("offscreen panel states and controls with host stubs", () => {
  const harness = preparePanelHarness(temporaryDirectory());
  const result = spawnSync(QUICKSHELL, ["--no-color", "-p", harness], {
    encoding: "utf8",
    env: headlessEnvironment(dirname(dirname(harness))),
    timeout: HARNESS_TIMEOUT_MS,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;

  assert.equal(result.status, 0, output);
  assert.match(output, SUCCESS_PATTERN);
});

test("plugin export records artifacts that match the export commit", () => {
  const root = temporaryDirectory();
  const nativeMeta = join(root, "native.json");
  const sourceMeta = join(root, "source.json");
  writeFileSync(
    nativeMeta,
    `${JSON.stringify({
      components: NATIVE_COMPONENTS,
      controlProtocol: CONTROL_PROTOCOL,
      schemaVersion: 2,
      sourceCommit: SOURCE_COMMIT,
      version: "0.0.0",
    })}\n`,
  );
  writeFileSync(
    sourceMeta,
    `${JSON.stringify({
      sha256: "c".repeat(SHA256_LENGTH),
      sourceCommit: SOURCE_COMMIT,
    })}\n`,
  );

  const artifacts = loadReleaseArtifacts({
    nativeMeta,
    sourceCommit: SOURCE_COMMIT,
    sourceMeta,
  });

  assert.deepEqual(artifacts.nativeComponents, NATIVE_COMPONENTS);
  assert.equal(artifacts.nativeVersion, "0.0.0");
  assert.equal(artifacts.sourceSha256, "c".repeat(SHA256_LENGTH));
});

test("plugin export rejects stale artifact commits", () => {
  const root = temporaryDirectory();
  const nativeMeta = join(root, "native.json");
  writeFileSync(
    nativeMeta,
    `${JSON.stringify({
      components: NATIVE_COMPONENTS,
      controlProtocol: CONTROL_PROTOCOL,
      schemaVersion: 2,
      sourceCommit: "c".repeat(COMMIT_LENGTH),
      version: "0.0.0",
    })}\n`,
  );

  assert.throws(
    () =>
      loadReleaseArtifacts({
        nativeMeta,
        sourceCommit: SOURCE_COMMIT,
      }),
    NATIVE_COMMIT_MISMATCH,
  );
});

test("plugin export rejects stale source-build commits", () => {
  const root = temporaryDirectory();
  const sourceMeta = join(root, "source.json");
  writeFileSync(
    sourceMeta,
    `${JSON.stringify({
      sha256: "c".repeat(SHA256_LENGTH),
      sourceCommit: "d".repeat(COMMIT_LENGTH),
    })}\n`,
  );

  assert.throws(
    () =>
      loadReleaseArtifacts({
        sourceCommit: SOURCE_COMMIT,
        sourceMeta,
      }),
    SOURCE_COMMIT_MISMATCH,
  );
});
