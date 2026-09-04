import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test, { afterEach } from "node:test";
import { fileURLToPath } from "node:url";

import { run } from "../../gates/lib/process.mjs";
import {
  ALLOWLIST_FILE,
  assertClosedTree,
  collectAllowlistedPaths,
  copyAllowlistedSources,
  loadAllowlist,
  readAppVersion,
  sparseCheckoutPatterns,
} from "../source-allowlist.mjs";
import {
  assertCleanReleaseInputs,
  createSourceBundle,
  extractAndBuild,
  materializeSparseCheckout,
} from "../source-build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const temporaryDirectories = new Set();
const COMMIT_LENGTH = 40;
const CONTROL_PROTOCOL = 3;
const SOURCE_COMMIT = "a".repeat(COMMIT_LENGTH);
const APP_MEMBER_PATTERN = /crates\/app/u;
const BLUEZ_MEMBER_PATTERN = /crates\/platform\/bluez/u;
const SUPPORT_MEMBER_PATTERN = /tests\/support/u;
const MISSING_LOCK_PATTERN = /missing allowlisted file: Cargo.lock/u;
const LEAKED_PATHS_PATTERN = /leaked paths/u;
const MEMBERS_BLOCK_PATTERN = /^members = \[(?<members>[^\]]*)\]/mu;
const MEMBER_PATH_PATTERN = /"(?<path>[^"]+)"/gu;
const EDITION_PATTERN = /edition = "2024"/u;
const LICENSE_PATTERN = /license = "Apache-2.0"/u;
const RUST_VERSION_PATTERN = /rust-version = "1.98"/u;
const SERDE_PATTERN = /serde = \{ version = "=1.0.228"/u;
const RESOLVER_PATTERN = /^resolver = "3"$/mu;
const DEFAULT_MEMBERS_ONLY_PATTERN =
  /^default-members = \[\n(?: {2}"crates\/[^"]+",\n)+\]$/mu;
const EXPECTED_PATTERNS = [
  "Cargo.lock",
  "Cargo.toml",
  "LICENSE",
  "README.md",
  "crates/app",
  "crates/core/connections",
  "crates/core/control",
  "crates/core/crypto",
  "crates/core/sharing",
  "crates/core/wire",
  "crates/platform/bluez",
  "crates/platform/network",
  "crates/platform/storage",
  "packaging/systemd/omarchy-quickshare.service",
  "packaging/systemd/omarchy-quickshare.toml",
  "rust-toolchain.toml",
];
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
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare daemon\n",
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
]
members = [
  "crates/app",
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
  mkdirSync(join(root, "tests"), { recursive: true });
  mkdirSync(join(root, "tools"), { recursive: true });
  mkdirSync(join(root, "upstream"), { recursive: true });
  writeFileSync(join(root, "tests", "suite.rs"), "");
  writeFileSync(join(root, "tools", "leak.mjs"), "");
  writeFileSync(join(root, "upstream", "sources.toml"), "");
}

function runReleaseCommand(calls) {
  return (command, args, options) => {
    const call = { args, command };
    if (options) {
      call.options = options;
    }
    calls.push(call);
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
]
members = [
  "crates/app",
  "tests/support",
]
`,
      );
    }
  };
}

test("one allowlist drives bundle and sparse-checkout path sets", () => {
  const allowlist = JSON.parse(readFileSync(ALLOWLIST_FILE, "utf8"));
  const workspace = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const membersMatch = workspace.match(MEMBERS_BLOCK_PATTERN);
  assert.notEqual(membersMatch, null);
  const { members } = membersMatch.groups;
  const runtime = [...members.matchAll(MEMBER_PATH_PATTERN)]
    .map((match) => match.groups.path)
    .filter((member) => member.startsWith("crates/"))
    .sort();
  assert.deepEqual(sparseCheckoutPatterns(), EXPECTED_PATTERNS);
  assert.deepEqual(
    allowlist.files.concat(allowlist.trees).sort(),
    EXPECTED_PATTERNS,
  );
  assert.deepEqual(allowlist.trees.slice().sort(), runtime);
});

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
  assert.match(workspace, APP_MEMBER_PATTERN);
  assert.match(workspace, BLUEZ_MEMBER_PATTERN);
  assert.doesNotMatch(workspace, SUPPORT_MEMBER_PATTERN);
  assert.match(workspace, EDITION_PATTERN);
  assert.match(workspace, LICENSE_PATTERN);
  assert.match(workspace, RUST_VERSION_PATTERN);
  assert.match(workspace, SERDE_PATTERN);
  assert.match(workspace, RESOLVER_PATTERN);
  assert.match(workspace, DEFAULT_MEMBERS_ONLY_PATTERN);
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

test("extracting the bundle builds the locked stripped binary", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  const destination = temporaryDirectory();
  const calls = [];
  const bundle = createSourceBundle({
    destination,
    root,
    runCommand: runReleaseCommand(calls),
    sourceCommit: SOURCE_COMMIT,
  });
  const workDirectory = join(temporaryDirectory(), "empty");
  mkdirSync(workDirectory);
  const built = extractAndBuild({
    archive: bundle.archive,
    runCommand: runReleaseCommand(calls),
    workDirectory,
  });
  const cargo = calls.filter((call) => call.command === "cargo");
  const meta = JSON.parse(
    readFileSync(join(destination, "version.json"), "utf8"),
  );

  assert.equal(built.version, "0.0.0");
  assert.equal(bundle.version, readAppVersion(root));
  assert.equal(built.version, bundle.version);
  assert.deepEqual(meta, {
    controlProtocol: CONTROL_PROTOCOL,
    sha256: bundle.sha256,
    sourceCommit: SOURCE_COMMIT,
    version: bundle.version,
  });
  assert.deepEqual(cargo[0].args, [
    "build",
    "--release",
    "--locked",
    "--package",
    "omarchy-quickshare",
  ]);
  assert.equal(cargo[0].options.cwd, workDirectory);
});

test("sparse-checkout materialization uses the shared allowlist", () => {
  const calls = [];
  const destination = join(temporaryDirectory(), "sparse");
  materializeSparseCheckout({
    destination,
    repository: "/tmp/repo",
    runCommand: runReleaseCommand(calls),
  });

  assert.deepEqual(calls[2].args, [
    "-C",
    destination,
    "sparse-checkout",
    "set",
    ...EXPECTED_PATTERNS,
  ]);
  const workspace = readFileSync(join(destination, "Cargo.toml"), "utf8");
  assert.match(workspace, APP_MEMBER_PATTERN);
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

test("clean release inputs pass before commit stamping", () => {
  const root = temporaryDirectory();
  writeRuntimeFiles(root);
  run("git", ["-C", root, "init", "-b", "main"]);
  commitAll(root, "init");

  assert.doesNotThrow(() =>
    assertCleanReleaseInputs({
      paths: sparseCheckoutPatterns(),
      root,
    }),
  );
});

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
