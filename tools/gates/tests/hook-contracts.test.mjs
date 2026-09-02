import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { packageSelection, parseAffectedJson } from "../../hooks/affected.mjs";
import { validateCommitMessage } from "../../hooks/commit-msg.mjs";
import { parseNameStatus } from "../../hooks/prepare-staged.mjs";
import { pushedCommits } from "../../hooks/pre-push.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const OVERLONG_SUBJECT_LENGTH = 70;
const SHA_LENGTH = 40;
const MALFORMED_COMMIT_TEST =
  "Conventional Commit validation rejects vague or malformed subjects";
const CARGO_SELECTION_TEST =
  "Cargo selection includes owners and transitive downstream packages";
const PRE_PUSH_ENVIRONMENT_TEST =
  "pre-push shares only the prepared test environment with exact worktrees";
const CONVENTIONAL_COMMIT_PATTERN = /Conventional Commits/u;
const SUBJECT_LENGTH_PATTERN = /72 characters/u;
const AST_GREP_ENV_PATTERN = /AST_GREP: join\(nodeBin, "ast-grep"\)/u;
const TEST_ENV_PATTERN = /TEST_ENV_CACHE: join\(ROOT, "\.cache", "test-env"\)/u;
const BROAD_CACHE_PATTERN = /TEST_ENV_CACHE: join\(ROOT, "\.cache"\)/u;

function cargoPackage(id, name, manifestPath) {
  return Object.fromEntries([
    ["id", id],
    ["name", name],
    ["manifest_path", manifestPath],
  ]);
}

test("Conventional Commit validation accepts the project types", () => {
  assert.equal(
    typeof validateCommitMessage("build(hooks): install staged checks\n"),
    "undefined",
  );
  assert.equal(
    typeof validateCommitMessage("feat(transfer)!: change frame version\n"),
    "undefined",
  );
});

test(MALFORMED_COMMIT_TEST, () => {
  assert.match(
    validateCommitMessage("setup hooks") ?? "",
    CONVENTIONAL_COMMIT_PATTERN,
  );
  assert.match(
    validateCommitMessage("build: install hooks.") ?? "",
    CONVENTIONAL_COMMIT_PATTERN,
  );
  assert.match(
    validateCommitMessage(`build: ${"a".repeat(OVERLONG_SUBJECT_LENGTH)}`) ??
      "",
    SUBJECT_LENGTH_PATTERN,
  );
});

test("staged status parsing preserves both sides of renames", () => {
  assert.deepEqual(
    parseNameStatus([
      "A",
      "new.rs",
      "R100",
      "old.rs",
      "renamed.rs",
      "D",
      "gone.rs",
    ]),
    [
      { status: "A", path: "new.rs" },
      { status: "R100", oldPath: "old.rs", path: "renamed.rs" },
      { status: "D", path: "gone.rs" },
    ],
  );
});

test("CodeGraph candidate parsing accepts nested JSON response shapes", () => {
  assert.deepEqual(
    parseAffectedJson({
      affected: [{ path: "tests/direct.rs" }, "tests/transitive.rs"],
    }).sort(),
    ["tests/direct.rs", "tests/transitive.rs"],
  );
});

test(CARGO_SELECTION_TEST, () => {
  const root = "/repo";
  const core = cargoPackage(
    "core 0.1.0",
    "core",
    "/repo/crates/core/Cargo.toml",
  );
  const app = cargoPackage("app 0.1.0", "app", "/repo/crates/app/Cargo.toml");
  const metadata = {
    packages: [core, app],
    ...Object.fromEntries([["workspace_members", [core.id, app.id]]]),
    resolve: {
      nodes: [
        { id: core.id, dependencies: [] },
        { id: app.id, dependencies: [core.id] },
      ],
    },
  };
  assert.deepEqual(
    packageSelection(metadata, ["crates/core/src/lib.rs"], root),
    [
      { name: "app", root: "crates/app" },
      { name: "core", root: "crates/core" },
    ],
  );
});

test("pre-push selects unique non-deletion tips", () => {
  const sha = "1".repeat(SHA_LENGTH);
  const deletion = "0".repeat(SHA_LENGTH);
  const input = [
    `refs/heads/main ${sha} refs/heads/main ${deletion}`,
    `refs/tags/v1 ${sha} refs/tags/v1 ${deletion}`,
    `refs/heads/old ${deletion} refs/heads/old ${sha}`,
  ].join("\n");
  assert.deepEqual(pushedCommits(input, "2".repeat(SHA_LENGTH)), [sha]);
  assert.deepEqual(pushedCommits("", sha), [sha]);
});

test(PRE_PUSH_ENVIRONMENT_TEST, () => {
  const source = readFileSync(
    join(ROOT, "tools", "hooks", "pre-push.mjs"),
    "utf8",
  );
  assert.match(source, AST_GREP_ENV_PATTERN);
  assert.match(source, TEST_ENV_PATTERN);
  assert.doesNotMatch(source, BROAD_CACHE_PATTERN);
});
