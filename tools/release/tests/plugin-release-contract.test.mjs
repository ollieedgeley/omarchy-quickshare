import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
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
import test, { afterEach } from "node:test";
import { fileURLToPath } from "node:url";

import {
  createPluginRepository,
  loadReleaseArtifacts,
} from "../plugin-export.mjs";
import { HARNESS_STUBS } from "./plugin-harness-stubs.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const STATUS_HARNESS = join(
  ROOT,
  "tools",
  "release",
  "tests",
  "status-harness.qml",
);
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
const QUICKSHELL = process.env.QUICKSHELL ?? "quickshell";
const QUICKSHELL_VERSION_PATTERN = /^Quickshell 0\.3\.1\b/u;
const SUCCESS_PATTERN = /HARNESS_OK/u;
const SOURCE_COMMIT = "a".repeat(COMMIT_LENGTH);
const SHA256_LENGTH = 64;
const CONTROL_PROTOCOL = 3;
const REJECTED_PROTOCOL = 4;
const EXPECTED_FILES = [
  "AttachmentBadge.qml",
  "BarWidget.qml",
  "ConsentView.qml",
  "LICENSE",
  "PeerChoiceView.qml",
  "README.md",
  "SharePanel.qml",
  "StatusProbe.qml",
  "TerminalView.qml",
  "TransferView.qml",
  "manifest.json",
  "release.json",
];
const PANEL_FILES = [
  "AttachmentBadge.qml",
  "ConsentView.qml",
  "PeerChoiceView.qml",
  "SharePanel.qml",
  "TerminalView.qml",
  "TransferView.qml",
];
const QML_FILES = ["BarWidget.qml", ...PANEL_FILES, "StatusProbe.qml"];
const FADE_DURATION_PATTERN = /duration: 1000/gu;
const FORBIDDEN_QML_COMMAND_PATTERN =
  /\b(?:bluetoothctl|cargo|curl|nmcli|pacman|paru|rsync|scp|wget|yay)\b/u;
const IPC_PASTE_FUNCTION_PATTERN =
  /function paste\(value: string\): string \{/u;
const TARGETED_SEND_ACTION_PATTERN =
  /runAction\(\[\s*"send",\s*"--peer",[\s\S]*String\(value\)/u;
const PASTE_FORWARD_PATTERN = /return root\.paste\(value\)/u;
const OPEN_DISCOVERY_PATTERN =
  /function open\(\) \{[\s\S]{0,200}status\.discover\(\)/u;
const CLIPBOARD_READ_PATTERN =
  /command: \["wl-paste", "--type", "text\/uri-list", "--no-newline"\]/u;
const CLOSED_PASTE_SUBMIT_PATTERN =
  /if \(opened\)[\s\S]{0,120}status\.submit\(value\)/u;
const STALE_PASTE_INSTRUCTION =
  /Paste while this panel is open to choose a nearby device\./u;
const SUBMIT_FUNCTION_PATTERN = /function submit\(value\) \{/u;
const KEYBOARD_PANEL_PATTERN = /\bKeyboardPanel\s*\{/u;
const POPUP_CARD_PATTERN = /\bPopupCard\s*\{/u;
const SHOW_PASTE_BADGE_PATTERN = /showPasteBadge:/u;
const ACTION_BUSY_PATTERN = /actionBusy:/u;
const MAX_QML_LINES = 500;
const EXECUTABLE_MODE = 0o755;
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

function prepareHarness(root) {
  const directory = join(root, "harness");
  mkdirSync(directory);
  for (const file of [...QML_FILES, "release.json"]) {
    copyFileSync(join(PLUGIN_SOURCE, file), join(directory, file));
  }
  writeHarnessStubs(directory);
  const nativeDirectory = join(root, "native");
  const actionLog = join(root, "actions.log");
  mkdirSync(nativeDirectory);
  const executable = join(nativeDirectory, "omarchy-quickshare");
  writeFileSync(
    executable,
    `#!/usr/bin/env bash
set -Eeuo pipefail
case "\${1-}" in
  protocol-version) printf '3' ;;
  health) ;;
  status)
    printf '%s' '{"response":{"type":"snapshot","snapshot":{}},"version":3}'
    ;;
  *)
    printf '%s\\n' "$*" >> "\${QUICKSHARE_TEST_LOG:?}"
    sleep 0.2
    ;;
esac
`,
  );
  chmodSync(executable, EXECUTABLE_MODE);
  const harness = join(directory, "status-harness.qml");
  copyFileSync(STATUS_HARNESS, harness);
  return { actionLog, harness, nativeDirectory };
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

test("plugin export contains only its allowlisted release files", () => {
  const root = temporaryDirectory();
  const destination = join(root, "export");

  createPluginRepository({ destination, sourceCommit: SOURCE_COMMIT });

  const files = readdirSync(destination)
    .filter((file) => file !== ".git")
    .sort();
  assert.deepEqual(files, EXPECTED_FILES);
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

test("plugin QML captures clipboard data and targets a selected peer", () => {
  for (const file of QML_FILES) {
    const source = readFileSync(join(PLUGIN_SOURCE, file), "utf8");
    assert.ok(
      source.trimEnd().split("\n").length <= MAX_QML_LINES,
      `${file} exceeds ${MAX_QML_LINES} lines`,
    );
    assert.doesNotMatch(source, FORBIDDEN_QML_COMMAND_PATTERN);
  }

  const bar = readFileSync(join(PLUGIN_SOURCE, "BarWidget.qml"), "utf8");
  assert.match(bar, IPC_PASTE_FUNCTION_PATTERN);
  assert.match(bar, PASTE_FORWARD_PATTERN);
  assert.match(bar, OPEN_DISCOVERY_PATTERN);
  assert.match(bar, CLIPBOARD_READ_PATTERN);
  assert.match(bar, CLOSED_PASTE_SUBMIT_PATTERN);
  assert.equal((bar.match(FADE_DURATION_PATTERN) ?? []).length, 2);
  assert.match(bar, KEYBOARD_PANEL_PATTERN);
  assert.doesNotMatch(bar, POPUP_CARD_PATTERN);
  assert.match(bar, SHOW_PASTE_BADGE_PATTERN);
  assert.match(bar, ACTION_BUSY_PATTERN);

  const panel = readFileSync(join(PLUGIN_SOURCE, "SharePanel.qml"), "utf8");
  assert.doesNotMatch(panel, STALE_PASTE_INSTRUCTION);

  const status = readFileSync(join(PLUGIN_SOURCE, "StatusProbe.qml"), "utf8");
  assert.match(status, SUBMIT_FUNCTION_PATTERN);
  assert.match(status, TARGETED_SEND_ACTION_PATTERN);
});

test("plugin export omits checksums when native artifacts are absent", () => {
  const source = join(temporaryDirectory(), "plugin");
  mkdirSync(source);
  for (const file of EXPECTED_FILES) {
    copyFileSync(join(PLUGIN_SOURCE, file), join(source, file));
  }
  const polluted = JSON.parse(
    readFileSync(join(source, "release.json"), "utf8"),
  );
  polluted.nativeArtifact.sha256 = "d".repeat(SHA256_LENGTH);
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
  assert.equal("sha256" in release.nativeArtifact, false);
  assert.deepEqual(release.controlProtocol, {
    minimum: CONTROL_PROTOCOL,
    maximum: CONTROL_PROTOCOL,
  });
  assert.equal("sourceBuild" in release, false);
});

test("plugin export records native and source-build checksums", () => {
  const destination = join(temporaryDirectory(), "export");
  const nativeSha256 = "b".repeat(SHA256_LENGTH);
  const sourceSha256 = "c".repeat(SHA256_LENGTH);

  createPluginRepository({
    artifacts: {
      nativeSha256,
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
  assert.equal(release.nativeArtifact.sha256, nativeSha256);
  assert.equal(release.nativeArtifact.published, true);
  assert.deepEqual(release.controlProtocol, {
    minimum: CONTROL_PROTOCOL,
    maximum: CONTROL_PROTOCOL,
  });
  assert.equal(release.sourceBuild.sha256, sourceSha256);
});

test("Quick Shell runtime matches the supported version", () => {
  const result = spawnSync(QUICKSHELL, ["--version"], { encoding: "utf8" });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;

  assert.equal(result.status, 0, output);
  assert.match(output, QUICKSHELL_VERSION_PATTERN);
});

test("Quick Shell exercises availability and busy paste integration", () => {
  const prepared = prepareHarness(temporaryDirectory());
  const result = spawnSync(QUICKSHELL, ["--no-color", "-p", prepared.harness], {
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${prepared.nativeDirectory}:${process.env.PATH ?? ""}`,
      QUICKSHARE_TEST_LOG: prepared.actionLog,
    },
    timeout: HARNESS_TIMEOUT_MS,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;

  assert.equal(result.status, 0, output);
  assert.match(output, SUCCESS_PATTERN);
  assert.equal(
    readFileSync(prepared.actionLog, "utf8").trim(),
    "send first-paste",
  );
});

test("Quick Shell renders safe transfer states and exact controls", () => {
  const harness = preparePanelHarness(temporaryDirectory());
  const result = spawnSync(QUICKSHELL, ["--no-color", "-p", harness], {
    encoding: "utf8",
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
      sha256: "b".repeat(SHA256_LENGTH),
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

  assert.equal(artifacts.nativeSha256, "b".repeat(SHA256_LENGTH));
  assert.equal(artifacts.nativeVersion, "0.0.0");
  assert.equal(artifacts.sourceSha256, "c".repeat(SHA256_LENGTH));
});

test("plugin export rejects stale artifact commits", () => {
  const root = temporaryDirectory();
  const nativeMeta = join(root, "native.json");
  writeFileSync(
    nativeMeta,
    `${JSON.stringify({
      sha256: "b".repeat(SHA256_LENGTH),
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
