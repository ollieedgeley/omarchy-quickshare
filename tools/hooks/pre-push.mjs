import { existsSync, mkdirSync, rmSync } from "node:fs";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";

const ROOT = output("git", ["rev-parse", "--show-toplevel"]);
const ZERO = /^0+$/u;
const WHITESPACE_PATTERN = /\s+/u;
const FIELD_COUNT = 4;

export function pushedCommits(input, fallbackHead) {
  const commits = new Set();
  for (const line of input.trim().split("\n").filter(Boolean)) {
    const fields = line.trim().split(WHITESPACE_PATTERN);
    if (fields.length !== FIELD_COUNT) {
      throw new Error(`invalid pre-push input: ${line}`);
    }
    if (!ZERO.test(fields[1])) {
      commits.add(fields[1]);
    }
  }
  if (!input.trim() && fallbackHead) {
    commits.add(fallbackHead);
  }
  return [...commits];
}

function readInput() {
  if (process.stdin.isTTY) {
    return "";
  }
  return new Promise((resolveInput) => {
    let value = "";
    process.stdin.setEncoding("utf8");
    process.stdin.on("data", (chunk) => {
      value += chunk;
    });
    process.stdin.on("end", () => resolveInput(value));
  });
}

function exactEnvironment() {
  const nodeBin = join(ROOT, "node_modules", ".bin");
  return {
    ...process.env,
    AST_GREP: join(nodeBin, "ast-grep"),
    CARGO_MACHETE: join(
      ROOT,
      ".cache",
      "tools",
      "cargo-machete-0.9.2",
      "bin",
      "cargo-machete",
    ),
    CARGO_TARGET_DIR: join(ROOT, "target"),
    CODEGRAPH: join(nodeBin, "codegraph"),
    NODE_BIN: nodeBin,
    PATH: `${nodeBin}:${process.env.PATH}`,
    RUFF: join(ROOT, ".cache", "tools", "ruff-0.16.5", "ruff"),
    TEST_ENV_CACHE: join(ROOT, ".cache", "test-env"),
    VULTURE: join(ROOT, ".cache", "tools", "vulture-2.16", "bin", "vulture"),
  };
}

export function verificationWorktree(cache) {
  const worktree = join(cache, "pre-push-worktree");
  const expectedPrefix = `${resolve(cache)}${sep}`;
  if (!resolve(worktree).startsWith(expectedPrefix)) {
    throw new Error(`refusing unsafe pre-push worktree path: ${worktree}`);
  }
  return worktree;
}

function cleanVerificationWorktree(worktree, allowFailure = false) {
  run("git", ["-C", worktree, "reset", "--hard", "HEAD"], {
    cwd: ROOT,
    allowFailure,
  });
  run("git", ["-C", worktree, "clean", "-ffdx"], {
    cwd: ROOT,
    allowFailure,
  });
}

function prepareVerificationWorktree(cache, safeCommit) {
  const worktree = verificationWorktree(cache);
  if (!existsSync(join(worktree, ".git"))) {
    run("git", ["worktree", "prune"], { cwd: ROOT });
    rmSync(worktree, { recursive: true, force: true });
    run("git", ["worktree", "add", "--detach", worktree, safeCommit], {
      cwd: ROOT,
    });
    return worktree;
  }
  cleanVerificationWorktree(worktree);
  run("git", ["-C", worktree, "checkout", "--detach", safeCommit], {
    cwd: ROOT,
  });
  return worktree;
}

async function main() {
  const input = await readInput();
  const head = output("git", ["rev-parse", "HEAD"], { cwd: ROOT });
  const commits = pushedCommits(input, head);
  const cache = join(ROOT, ".cache", "gates");
  mkdirSync(cache, { recursive: true });

  for (const commit of commits) {
    const safeCommit = output("git", ["rev-parse", `${commit}^{commit}`], {
      cwd: ROOT,
    });
    const worktree = prepareVerificationWorktree(cache, safeCommit);
    try {
      const env = exactEnvironment();
      run("make", ["verify"], { cwd: worktree, env });
      run("make", ["build"], { cwd: worktree, env });
    } finally {
      cleanVerificationWorktree(worktree, true);
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}
