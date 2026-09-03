import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  lstatSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test, { afterEach } from "node:test";
import { fileURLToPath } from "node:url";

import { createPluginRepository } from "../plugin-export.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const HARNESS = join(ROOT, "tools", "release", "tests", "status-harness.qml");
const PLUGIN_SOURCE = join(ROOT, "packaging", "omarchy-plugin");
const COMMIT_LENGTH = 40;
const HARNESS_TIMEOUT_MS = 8_000;
const QUICKSHELL = process.env.QUICKSHELL ?? "quickshell";
const QUICKSHELL_VERSION_PATTERN = /^Quickshell 0\.3\.1\b/u;
const SUCCESS_PATTERN = /HARNESS_OK/u;
const SOURCE_COMMIT = "a".repeat(COMMIT_LENGTH);
const EXPECTED_FILES = [
  "BarWidget.qml",
  "LICENSE",
  "README.md",
  "StatusProbe.qml",
  "manifest.json",
  "release.json",
];
const temporaryDirectories = new Set();

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-plugin-"));
  temporaryDirectories.add(directory);
  return directory;
}

function prepareHarness(root) {
  const directory = join(root, "harness");
  mkdirSync(directory);
  for (const file of ["StatusProbe.qml", "release.json"]) {
    copyFileSync(join(PLUGIN_SOURCE, file), join(directory, file));
  }
  const harness = join(directory, "status-harness.qml");
  copyFileSync(HARNESS, harness);
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
});

test("Quick Shell runtime matches the supported version", () => {
  const result = spawnSync(QUICKSHELL, ["--version"], { encoding: "utf8" });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;

  assert.equal(result.status, 0, output);
  assert.match(output, QUICKSHELL_VERSION_PATTERN);
});

test("Quick Shell observes every native availability state", () => {
  const harness = prepareHarness(temporaryDirectory());
  const result = spawnSync(QUICKSHELL, ["--no-color", "-p", harness], {
    encoding: "utf8",
    timeout: HARNESS_TIMEOUT_MS,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;

  assert.equal(result.status, 0, output);
  assert.match(output, SUCCESS_PATTERN);
});
