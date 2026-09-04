import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SOURCE = join(ROOT, "packaging", "omarchy-plugin");
const DESTINATION = join(ROOT, "dist", "omarchy-plugin");
const COMMIT_PATTERN = /^[0-9a-f]{40}$/u;
const PLUGIN_FILES = [
  "BarWidget.qml",
  "LICENSE",
  "README.md",
  "SharePanel.qml",
  "StatusProbe.qml",
  "manifest.json",
  "release.json",
];

function assertSourceFile(source, file) {
  const path = join(source, file);
  const metadata = lstatSync(path);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(`plugin source is not a regular file: ${file}`);
  }
  return path;
}

function writeRelease(destination, sourceCommit, artifacts) {
  const path = join(destination, "release.json");
  const release = JSON.parse(readFileSync(path, "utf8"));
  release.sourceCommit = sourceCommit;
  release.controlProtocol = { maximum: 2, minimum: 2 };
  const nativeArtifact = {
    version: artifacts.nativeVersion ?? release.nativeArtifact.version,
    published: Boolean(artifacts.nativeSha256),
  };
  if (artifacts.nativeSha256) {
    nativeArtifact.sha256 = artifacts.nativeSha256;
  }
  release.nativeArtifact = nativeArtifact;
  if (artifacts.sourceSha256) {
    release.sourceBuild = { sha256: artifacts.sourceSha256 };
  } else {
    delete release.sourceBuild;
  }
  writeFileSync(path, `${JSON.stringify(release, null, 2)}\n`);
}

export function exportPlugin({
  artifacts = {},
  destination,
  source = SOURCE,
  sourceCommit,
}) {
  if (!COMMIT_PATTERN.test(sourceCommit)) {
    throw new Error("plugin export needs a full source commit");
  }
  if (existsSync(destination)) {
    throw new Error(`plugin export already exists: ${destination}`);
  }
  mkdirSync(destination, { recursive: true });
  for (const file of PLUGIN_FILES) {
    copyFileSync(assertSourceFile(source, file), join(destination, file));
  }
  writeRelease(destination, sourceCommit, artifacts);
}

export function createPluginRepository(options) {
  exportPlugin(options);
  const { destination, sourceCommit } = options;
  run("git", ["-C", destination, "init", "-b", "main"]);
  run("git", ["-C", destination, "add", "--all"]);
  run("git", [
    "-C",
    destination,
    "-c",
    "user.name=Omarchy Quick Share local export",
    "-c",
    "user.email=local-export@invalid",
    "commit",
    "-m",
    `build: export plugin from ${sourceCommit}`,
  ]);
}

function cleanDestination(destination) {
  if (resolve(destination) !== DESTINATION) {
    throw new Error("refusing to clean an unexpected export path");
  }
  rmSync(destination, { force: true, recursive: true });
}

function assertMatchingCommit(label, meta, sourceCommit) {
  if (meta.sourceCommit !== sourceCommit) {
    throw new Error(`${label} artifact sourceCommit does not match export`);
  }
}

export function loadReleaseArtifacts({
  nativeMeta,
  sourceCommit,
  sourceMeta,
} = {}) {
  if (!COMMIT_PATTERN.test(sourceCommit)) {
    throw new Error("plugin export needs a full source commit");
  }
  const artifacts = {};
  if (nativeMeta && existsSync(nativeMeta)) {
    const native = JSON.parse(readFileSync(nativeMeta, "utf8"));
    assertMatchingCommit("native", native, sourceCommit);
    artifacts.nativeSha256 = native.sha256;
    artifacts.nativeVersion = native.version;
  }
  if (sourceMeta && existsSync(sourceMeta)) {
    const source = JSON.parse(readFileSync(sourceMeta, "utf8"));
    assertMatchingCommit("source", source, sourceCommit);
    artifacts.sourceSha256 = source.sha256;
  }
  return artifacts;
}

function main() {
  run("git", ["diff", "--quiet", "HEAD", "--", "packaging/omarchy-plugin"]);
  const sourceCommit = output("git", ["rev-parse", "HEAD"]);
  cleanDestination(DESTINATION);
  createPluginRepository({
    artifacts: loadReleaseArtifacts({
      nativeMeta: join(ROOT, "dist", "native", "version.json"),
      sourceCommit,
      sourceMeta: join(ROOT, "dist", "source", "version.json"),
    }),
    destination: DESTINATION,
    sourceCommit,
  });
  run("omarchy", ["plugin", "validate", DESTINATION]);
  process.stdout.write(`Exported plugin for ${sourceCommit}.\n`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
