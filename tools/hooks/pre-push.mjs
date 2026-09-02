import { mkdirSync, rmSync } from "node:fs";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";

const ROOT = output("git", ["rev-parse", "--show-toplevel"]);
const ZERO = /^0+$/u;
const WHITESPACE_PATTERN = /\s+/u;
const FIELD_COUNT = 4;
const SHORT_HASH_LENGTH = 12;

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
    const worktree = join(
      cache,
      `pre-push-${process.pid}-${safeCommit.slice(0, SHORT_HASH_LENGTH)}`,
    );
    const expectedPrefix = `${resolve(cache)}${sep}`;
    if (!resolve(worktree).startsWith(expectedPrefix)) {
      throw new Error(`refusing unsafe pre-push worktree path: ${worktree}`);
    }
    rmSync(worktree, { recursive: true, force: true });
    run("git", ["worktree", "add", "--detach", worktree, safeCommit], {
      cwd: ROOT,
    });
    try {
      const nodeBin = join(ROOT, "node_modules", ".bin");
      const env = {
        ...process.env,
        AST_GREP: join(nodeBin, "ast-grep"),
        CODEGRAPH: join(nodeBin, "codegraph"),
        NODE_BIN: nodeBin,
        PATH: `${nodeBin}:${process.env.PATH}`,
        TEST_ENV_CACHE: join(ROOT, ".cache", "test-env"),
      };
      run("make", ["verify"], { cwd: worktree, env });
      run("make", ["build"], { cwd: worktree, env });
    } finally {
      run("git", ["worktree", "remove", "--force", worktree], {
        cwd: ROOT,
        allowFailure: true,
      });
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}
