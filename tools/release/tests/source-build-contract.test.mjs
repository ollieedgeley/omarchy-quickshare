import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "vitest";

import { run } from "../../gates/lib/process.mjs";
import {
  assertClosedTree,
  collectAllowlistedPaths,
  copyAllowlistedSources,
  loadAllowlist,
  sparseCheckoutPatterns,
} from "../source-allowlist.mjs";
import {
  assertCleanReleaseInputs,
  createSourceBundle,
  extractAndBuild,
  materializeSparseCheckout,
} from "../source-build.mjs";

const temporaryDirectories = new Set();
const COMMIT_LENGTH = 40;
const CONTROL_PROTOCOL = 7;
const SOURCE_COMMIT = "a".repeat(COMMIT_LENGTH);
const SUPPORT_MEMBER_PATTERN = /tests\/support/u;
const MISSING_LOCK_PATTERN = /missing allowlisted file: Cargo.lock/u;
const LEAKED_PATHS_PATTERN = /leaked paths/u;
const DIRTY_INPUTS_PATTERN = /release inputs are dirty/u;

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-source-"));
  temporaryDirectories.add(directory);
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories) {
    rmSync(directory, { force: true, recursive: true });
  }
  temporaryDirectories.clear();
});

function writeLeanDocs(root) {
  writeFileSync(join(root, "README.md"), "# src\n");
  mkdirSync(join(root, "packaging", "systemd"), { recursive: true });
  writeFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.service"),
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare-daemon\n",
  );
  writeFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.toml"),
    "# default\n",
  );
}

function writeRuntimeFiles(root) {
  writeFileSync(
    join(root, "Cargo.toml"),
    `[workspace]
default-members = [
  "tests/support",
  "crates/app",
  "crates/cli",
]
members = [
  "crates/app",
  "crates/cli",
  "tests/support",
]
resolver = "3"

[workspace.package]
edition = "2024"
license = "Apache-2.0"
rust-version = "1.98"

[workspace.dependencies]
serde = { version = "=1.0.228", features = ["derive"] }
`,
  );
  writeFileSync(join(root, "Cargo.lock"), "# lock\n");
  writeFileSync(join(root, "LICENSE"), "license\n");
  writeFileSync(join(root, "rust-toolchain.toml"), "[toolchain]\n");
  writeLeanDocs(root);
  for (const tree of loadAllowlist().trees) {
    mkdirSync(join(root, tree, "src"), { recursive: true });
    mkdirSync(join(root, tree, "tests"), { recursive: true });
    writeFileSync(join(root, tree, "Cargo.toml"), "[package]\n");
    writeFileSync(join(root, tree, "src", "lib.rs"), "");
    writeFileSync(join(root, tree, "tests", "leaked.rs"), "");
  }
  writeFileSync(
    join(root, "crates", "app", "Cargo.toml"),
    '[package]\nname = "omarchy-quickshare"\nversion = "0.0.0"\n',
  );
  writeFileSync(
    join(root, "crates", "cli", "Cargo.toml"),
    '[package]\nname = "omarchy-quickshare-cli"\nversion = "0.0.0"\n',
  );
  mkdirSync(join(root, "tests"), { recursive: true });
  mkdirSync(join(root, "tools"), { recursive: true });
  mkdirSync(join(root, "upstream"), { recursive: true });
  writeFileSync(join(root, "tests", "suite.rs"), "");
  writeFileSync(join(root, "tools", "leak.mjs"), "");
  writeFileSync(join(root, "upstream", "sources.toml"), "");
}

function runReleaseCommand() {
  return (command, args, options) => {
    if (args[0] === "protocol-version" || args[0] === "--protocol-version") {
      return { stdout: String(CONTROL_PROTOCOL) };
    }
    if (command === "cargo" && args[0] === "build") {
      const releaseDirectory = join(options.cwd, "target", "release");
      mkdirSync(releaseDirectory, { recursive: true });
      writeFileSync(join(releaseDirectory, "omarchy-quickshare"), "cli");
      writeFileSync(
        join(releaseDirectory, "omarchy-quickshare-daemon"),
        "daemon",
      );
    }
    if (command === "tar") {
      run(command, args, options);
    }
    if (command === "git" && args[0] === "clone") {
      const destination = args[args.length - 1];
      mkdirSync(destination, { recursive: true });
      writeFileSync(
        join(destination, "Cargo.toml"),
        `[workspace]
default-members = [
  "tests/support",
  "crates/app",
  "crates/cli",
]
members = [
  "crates/app",
  "crates/cli",
  "tests/support",
]
`,
      );
    }
    return null;
  };
}

test("source-build copies omit tests tools upstream and crate tests", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  const destination = join(temporaryDirectory(), "tree");
  mkdirSync(destination);
  const paths = copyAllowlistedSources(root, destination);
  const workspace = readFileSync(join(destination, "Cargo.toml"), "utf8");

  assert.equal(
    paths.some((path) => path.includes("/tests/")),
    false,
  );
  assert.equal(
    paths.some((path) => path.startsWith("tests/")),
    false,
  );
  assert.equal(
    paths.some((path) => path.startsWith("tools/")),
    false,
  );
  assert.equal(
    paths.some((path) => path.startsWith("upstream/")),
    false,
  );
  assert.equal(
    readFileSync(join(destination, "Cargo.lock"), "utf8"),
    "# lock\n",
  );
  assert.doesNotMatch(workspace, SUPPORT_MEMBER_PATTERN);
  assert.equal(readFileSync(join(destination, "README.md"), "utf8"), "# src\n");
  assert.equal(
    readFileSync(
      join(destination, "packaging", "systemd", "omarchy-quickshare.toml"),
      "utf8",
    ),
    "# default\n",
  );
  assertClosedTree(destination);
});

test("missing allowlisted inputs fail the path set contract", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  rmSync(join(root, "Cargo.lock"));

  assert.throws(() => collectAllowlistedPaths(root), MISSING_LOCK_PATTERN);
});

test("leaked denied paths fail clean build closure", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  const destination = join(temporaryDirectory(), "tree");
  mkdirSync(destination);
  copyAllowlistedSources(root, destination);
  mkdirSync(join(destination, "tools"));
  writeFileSync(join(destination, "tools", "leak.mjs"), "");

  assert.throws(() => assertClosedTree(destination), LEAKED_PATHS_PATTERN);
});

test("source bundle metadata survives extraction with simulated builds", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  const destination = temporaryDirectory();
  const bundle = createSourceBundle({
    destination,
    root,
    runCommand: runReleaseCommand(),
    sourceCommit: SOURCE_COMMIT,
  });
  const workDirectory = join(temporaryDirectory(), "empty");
  mkdirSync(workDirectory);
  const built = extractAndBuild({
    archive: bundle.archive,
    runCommand: runReleaseCommand(),
    workDirectory,
  });
  const meta = JSON.parse(
    readFileSync(join(destination, "version.json"), "utf8"),
  );

  assert.equal(built.version, "0.0.0");
  assert.equal(built.version, bundle.version);
  assert.deepEqual(meta, {
    controlProtocol: CONTROL_PROTOCOL,
    sha256: bundle.sha256,
    sourceCommit: SOURCE_COMMIT,
    version: bundle.version,
  });
});

test("simulated sparse checkout removes test dependencies", () => {
  const destination = join(temporaryDirectory(), "sparse");
  materializeSparseCheckout({
    destination,
    repository: "/tmp/repo",
    runCommand: runReleaseCommand(),
  });

  const workspace = readFileSync(join(destination, "Cargo.toml"), "utf8");
  assert.doesNotMatch(workspace, SUPPORT_MEMBER_PATTERN);
});

function commitAll(root, message) {
  run("git", ["-C", root, "add", "--all"]);
  run("git", [
    "-C",
    root,
    "-c",
    "user.name=Omarchy Quick Share test",
    "-c",
    "user.email=test@invalid",
    "commit",
    "-m",
    message,
  ]);
}

test("dirty tracked release inputs fail closed", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  run("git", ["-C", root, "init", "-b", "main"]);
  commitAll(root, "init");
  writeFileSync(join(root, "LICENSE"), "dirty\n");

  assert.throws(
    () =>
      assertCleanReleaseInputs({
        paths: sparseCheckoutPatterns(),
        root,
      }),
    DIRTY_INPUTS_PATTERN,
  );
});

test("untracked release inputs fail closed", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  run("git", ["-C", root, "init", "-b", "main"]);
  commitAll(root, "init");
  writeFileSync(join(root, "crates", "app", "src", "extra.rs"), "");

  assert.throws(
    () =>
      assertCleanReleaseInputs({
        paths: sparseCheckoutPatterns(),
        root,
      }),
    DIRTY_INPUTS_PATTERN,
  );
});
