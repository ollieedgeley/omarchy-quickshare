import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const PREPARE = join(ROOT, "tools", "hooks", "prepare-staged.mjs");
const RUN_STAGED = join(ROOT, "tools", "hooks", "run-staged.mjs");
const CODEGRAPH =
  process.env.CODEGRAPH ?? join(ROOT, "node_modules", ".bin", "codegraph");
const EXECUTABLE_MODE = 0o755;
const HOOK_SETUP_PATTERN = /make hooks-install/u;
const PARTIAL_STAGING_PATTERN = /staged and unstaged changes/u;

function runWithOptions(command, args, options) {
  const { cwd, extraEnvironment = {} } = options;
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, CODEGRAPH, ...extraEnvironment },
  });
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed:\n${result.stdout}${result.stderr}`,
    );
  }
  return result;
}

function run(command, args, cwd) {
  return runWithOptions(command, args, { cwd });
}

function fakeEslint(root) {
  const bin = join(root, ".fake-bin");
  const path = join(bin, "eslint");
  mkdirSync(bin, { recursive: true });
  writeFileSync(
    path,
    [
      "#!/usr/bin/env node",
      'const { readFileSync } = require("node:fs");',
      "const paths = process.argv.slice(2)" +
        ".filter((arg) => arg.endsWith('.mjs'));",
      "const failed = paths.some((file) =>",
      "  readFileSync(file, 'utf8').includes('forbidden'),",
      ");",
      "process.exitCode = failed ? 1 : 0;",
      "",
    ].join("\n"),
  );
  chmodSync(path, EXECUTABLE_MODE);
  return bin;
}

function repository() {
  const root = mkdtempSync(join(tmpdir(), "quickshare-staged-contract-"));
  run("git", ["init", "-b", "main"], root);
  run("git", ["config", "user.name", "Hook Contract"], root);
  run("git", ["config", "user.email", "hook@example.invalid"], root);
  return root;
}

test("staged mirror uses index bytes and reuses its CodeGraph database", () => {
  const root = repository();
  try {
    writeFileSync(join(root, "value.txt"), "staged\n");
    run("git", ["add", "value.txt"], root);
    writeFileSync(join(root, "value.txt"), "working tree\n");

    run("node", [PREPARE, "--initialize"], root);
    const mirror = join(root, ".cache", "gates", "pre-commit-tree");
    assert.equal(readFileSync(join(mirror, "value.txt"), "utf8"), "staged\n");
    assert.equal(existsSync(join(mirror, ".codegraph")), true);

    run("node", [PREPARE], root);
    assert.equal(readFileSync(join(mirror, "value.txt"), "utf8"), "staged\n");

    rmSync(join(mirror, ".codegraph"), { recursive: true, force: true });
    const missing = spawnSync("node", [PREPARE], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, CODEGRAPH },
    });
    assert.notEqual(missing.status, 0);
    assert.match(`${missing.stdout}${missing.stderr}`, HOOK_SETUP_PATTERN);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("staged mirror rejects partially staged Rust files", () => {
  const root = repository();
  try {
    writeFileSync(join(root, "sample.rs"), "pub fn staged() {}\n");
    run("git", ["add", "sample.rs"], root);
    writeFileSync(join(root, "sample.rs"), "pub fn working_tree() {}\n");
    const result = spawnSync("node", [PREPARE, "--initialize"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, CODEGRAPH },
    });
    assert.notEqual(result.status, 0);
    assert.match(`${result.stdout}${result.stderr}`, PARTIAL_STAGING_PATTERN);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("staged JavaScript gate reads index bytes and propagates failures", () => {
  const root = repository();
  try {
    const nodeBin = fakeEslint(root);
    const sample = join(root, "sample.mjs");
    writeFileSync(sample, "export const staged = true;\n");
    run("git", ["add", "sample.mjs"], root);
    writeFileSync(sample, "forbidden working tree\n");
    run("node", [PREPARE, "--initialize"], root);
    runWithOptions("node", [RUN_STAGED, "javascript"], {
      cwd: root,
      extraEnvironment: { NODE_BIN: nodeBin },
    });

    run("git", ["add", "sample.mjs"], root);
    run("node", [PREPARE], root);
    const failed = spawnSync("node", [RUN_STAGED, "javascript"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, NODE_BIN: nodeBin },
    });
    assert.notEqual(failed.status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
