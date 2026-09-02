import {
  existsSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";

const ROOT = output("git", ["rev-parse", "--show-toplevel"]);
const CACHE = join(ROOT, ".cache", "gates");
const MIRROR = join(CACHE, "pre-commit-tree");
const METADATA = join(CACHE, "staged.json");
const EMPTY_TREE = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const RENAME_FIELD_COUNT = 3;
const CODEGRAPH =
  process.env.CODEGRAPH ?? join(ROOT, "node_modules", ".bin", "codegraph");

function rawGit(args, allowFailure = false) {
  return run("git", args, {
    allowFailure,
    capture: true,
    cwd: ROOT,
    quiet: true,
  });
}

function nulFields(args) {
  const result = rawGit(args);
  return result.stdout.split("\0").filter(Boolean);
}

export function parseNameStatus(fields) {
  const changes = [];
  let index = 0;
  while (index < fields.length) {
    const status = fields.at(index);
    const code = status.charAt(0);
    if (code === "R" || code === "C") {
      changes.push({
        oldPath: fields.at(index + 1),
        path: fields.at(index + 2),
        status,
      });
      index += RENAME_FIELD_COUNT;
    } else {
      changes.push({ path: fields.at(index + 1), status });
      index += 2;
    }
  }
  return changes;
}

function assertSafeMirror() {
  const expected = resolve(ROOT, ".cache", "gates", "pre-commit-tree");
  if (
    resolve(MIRROR) !== expected ||
    !expected.startsWith(`${resolve(ROOT)}${sep}`)
  ) {
    throw new Error(`refusing to refresh unsafe staged mirror path: ${MIRROR}`);
  }
}

function clearMirrorSources() {
  assertSafeMirror();
  mkdirSync(MIRROR, { recursive: true });
  for (const entry of readdirSync(MIRROR)) {
    if (entry !== ".codegraph") {
      rmSync(join(MIRROR, entry), { recursive: true, force: true });
    }
  }
}

function verifyMirror(paths) {
  for (const path of paths) {
    const stagedHash = output("git", ["rev-parse", `:${path}`], { cwd: ROOT });
    const mirrorPath = join(MIRROR, path);
    if (!existsSync(mirrorPath)) {
      throw new Error(`staged mirror is missing ${path}`);
    }
    const mirrorHash = output("git", ["hash-object", mirrorPath], {
      cwd: ROOT,
    });
    if (stagedHash !== mirrorHash) {
      throw new Error(
        `staged mirror content differs from the index for ${path}`,
      );
    }
  }
}

function assertNoPartialRustFiles(changes) {
  const stagedRust = new Set(
    changes
      .flatMap((change) => [change.path, change.oldPath])
      .filter((path) => path?.endsWith(".rs")),
  );
  const unstaged = new Set(nulFields(["diff", "--name-only", "-z"]));
  const partial = [...stagedRust].filter((path) => unstaged.has(path));
  if (partial.length) {
    throw new Error(
      "Rust files have both staged and unstaged changes: " +
        `${partial.join(", ")}. ` +
        "Stage the complete file or separate the changes before committing.",
    );
  }
}

function ensureCodeGraph(initialize) {
  const indexPath = join(MIRROR, ".codegraph");
  if (!existsSync(indexPath)) {
    if (!initialize) {
      throw new Error(
        "staged CodeGraph index is missing; run `make hooks-install`",
      );
    }
    run(CODEGRAPH, ["init", "--yes", MIRROR], { cwd: ROOT });
  }
  const version = output(CODEGRAPH, ["--version"], { cwd: ROOT });
  if (!version.includes("1.6.0")) {
    throw new Error(`expected CodeGraph 1.6.0, received ${version}`);
  }
  run(CODEGRAPH, ["sync", "--quiet", MIRROR], { cwd: ROOT });
}

function main() {
  const initialize = process.argv.includes("--initialize");
  mkdirSync(CACHE, { recursive: true });
  const head = rawGit(["rev-parse", "--verify", "HEAD"], true);
  let base = EMPTY_TREE;
  if (head.status === 0) {
    base = "HEAD";
  }
  const changes = parseNameStatus(
    nulFields([
      "diff",
      "--cached",
      "--name-status",
      "-z",
      "--find-renames",
      base,
    ]),
  );

  if (!initialize && changes.length === 0) {
    throw new Error("nothing is staged for commit");
  }
  run("git", ["diff", "--cached", "--check"], { cwd: ROOT });
  assertNoPartialRustFiles(changes);
  clearMirrorSources();
  run("git", ["checkout-index", "--all", "--force", `--prefix=${MIRROR}/`], {
    cwd: ROOT,
  });
  const stagedPaths = nulFields(["ls-files", "-z"]);
  verifyMirror(stagedPaths);
  ensureCodeGraph(initialize);

  const tree = output("git", ["write-tree"], { cwd: ROOT });
  writeFileSync(
    METADATA,
    `${JSON.stringify(
      { changes, mirror: MIRROR, root: ROOT, tree },
      null,
      2,
    )}\n`,
  );
  let action = "Refreshed";
  if (initialize) {
    action = "Initialized";
  }
  process.stdout.write(
    `${action} staged mirror for ${changes.length} change(s).\n`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
