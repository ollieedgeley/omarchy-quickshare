import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";
import {
  assertClosedTree,
  copyAllowlistedSources,
  readAppVersion,
  rewriteRuntimeWorkspace,
  sparseCheckoutPatterns,
} from "./source-allowlist.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DESTINATION = join(ROOT, "dist", "source");
const SPARSE_DESTINATION = join(ROOT, "dist", "sparse");
const COMMIT_PATTERN = /^[0-9a-f]{40}$/u;
const CONTROL_PROTOCOL = 3;

export function hashFile(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function assertCleanReleaseInputs({
  outputCommand = output,
  paths = sparseCheckoutPatterns(),
  root = ROOT,
  runCommand = run,
} = {}) {
  const diff = runCommand(
    "git",
    ["-C", root, "diff", "--quiet", "HEAD", "--", ...paths],
    { allowFailure: true },
  );
  const untracked = outputCommand("git", [
    "-C",
    root,
    "ls-files",
    "--others",
    "--exclude-standard",
    "--",
    ...paths,
  ]);
  if (diff.status !== 0 || untracked.length > 0) {
    throw new Error("release inputs are dirty");
  }
}

export function buildLockedBinary({ root, runCommand = run }) {
  runCommand(
    "cargo",
    ["build", "--release", "--locked", "--package", "omarchy-quickshare"],
    { cwd: root },
  );
  const binary = join(root, "target", "release", "omarchy-quickshare");
  runCommand("strip", ["--strip-unneeded", binary]);
  return { binary, version: readAppVersion(root) };
}

export function createSourceBundle({
  destination,
  root = ROOT,
  runCommand = run,
  sourceCommit,
} = {}) {
  if (!COMMIT_PATTERN.test(sourceCommit)) {
    throw new Error("source bundle needs a full source commit");
  }
  const tree = join(destination, "tree");
  rmSync(tree, { force: true, recursive: true });
  mkdirSync(tree, { recursive: true });
  const paths = copyAllowlistedSources(root, tree);
  assertClosedTree(tree);
  const archive = join(
    destination,
    `omarchy-quickshare-source-${sourceCommit}.tar.gz`,
  );
  runCommand("tar", ["-czf", archive, "-C", tree, "."]);
  const version = readAppVersion(root);
  const sha256 = hashFile(archive);
  const meta = {
    controlProtocol: CONTROL_PROTOCOL,
    sha256,
    sourceCommit,
    version,
  };
  writeFileSync(
    join(destination, "version.json"),
    `${JSON.stringify(meta, null, 2)}\n`,
  );
  return { archive, paths, sha256, tree, version };
}

export function extractAndBuild({ archive, runCommand = run, workDirectory }) {
  if (!existsSync(workDirectory)) {
    mkdirSync(workDirectory, { recursive: true });
  }
  if (readdirSync(workDirectory).length > 0) {
    throw new Error("source-build extract requires an empty directory");
  }
  runCommand("tar", ["-xzf", archive, "-C", workDirectory]);
  assertClosedTree(workDirectory);
  return buildLockedBinary({ root: workDirectory, runCommand });
}

export function materializeSparseCheckout({
  destination,
  repository,
  runCommand = run,
}) {
  if (existsSync(destination)) {
    throw new Error(`sparse checkout already exists: ${destination}`);
  }
  runCommand("git", [
    "clone",
    "--filter=blob:none",
    "--sparse",
    "--depth",
    "1",
    repository,
    destination,
  ]);
  runCommand("git", [
    "-C",
    destination,
    "sparse-checkout",
    "init",
    "--no-cone",
  ]);
  runCommand("git", [
    "-C",
    destination,
    "sparse-checkout",
    "set",
    ...sparseCheckoutPatterns(),
  ]);
  rewriteRuntimeWorkspace(destination);
  return sparseCheckoutPatterns();
}

function cleanKnown(destination, expected) {
  if (resolve(destination) !== expected) {
    throw new Error("refusing to clean an unexpected release path");
  }
  rmSync(destination, { force: true, recursive: true });
}

function bundleFromHead() {
  assertCleanReleaseInputs();
  const sourceCommit = output("git", ["rev-parse", "HEAD"]);
  cleanKnown(DESTINATION, DESTINATION);
  mkdirSync(DESTINATION, { recursive: true });
  const result = createSourceBundle({
    destination: DESTINATION,
    sourceCommit,
  });
  const built = extractAndBuild({
    archive: result.archive,
    workDirectory: join(DESTINATION, "extract"),
  });
  if (built.version !== result.version) {
    throw new Error("source-build version does not match the bundle");
  }
  process.stdout.write(`Source-build ${result.version} for ${sourceCommit}.\n`);
}

function main() {
  const mode = process.argv[2] ?? "bundle";
  if (mode === "bundle") {
    bundleFromHead();
    return;
  }
  if (mode === "sparse") {
    const sourceCommit = output("git", ["rev-parse", "HEAD"]);
    cleanKnown(SPARSE_DESTINATION, SPARSE_DESTINATION);
    materializeSparseCheckout({
      destination: SPARSE_DESTINATION,
      repository: ROOT,
    });
    assertClosedTree(SPARSE_DESTINATION);
    process.stdout.write(`Sparse checkout for ${sourceCommit}.\n`);
    return;
  }
  throw new Error("usage: source-build.mjs [bundle|sparse]");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
