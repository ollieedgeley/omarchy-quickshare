import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  existsSync,
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
const CODEGRAPH =
  process.env.CODEGRAPH ?? join(ROOT, "node_modules", ".bin", "codegraph");

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: { ...process.env, CODEGRAPH },
  });
  if (result.status !== 0) {
    throw new Error(
      `${command} ${args.join(" ")} failed:\n${result.stdout}${result.stderr}`,
    );
  }
  return result;
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
    assert.match(`${missing.stdout}${missing.stderr}`, /make hooks-install/);
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
    assert.match(
      `${result.stdout}${result.stderr}`,
      /staged and unstaged changes/,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
