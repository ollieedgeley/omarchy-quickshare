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

test("Conventional Commit validation accepts the project types", () => {
  assert.equal(
    validateCommitMessage("build(hooks): install staged checks\n"),
    undefined,
  );
  assert.equal(
    validateCommitMessage("feat(transfer)!: change frame version\n"),
    undefined,
  );
});

test("Conventional Commit validation rejects vague or malformed subjects", () => {
  assert.match(
    validateCommitMessage("setup hooks") ?? "",
    /Conventional Commits/,
  );
  assert.match(
    validateCommitMessage("build: install hooks.") ?? "",
    /Conventional Commits/,
  );
  assert.match(
    validateCommitMessage(`build: ${"a".repeat(70)}`) ?? "",
    /72 characters/,
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

test("Cargo selection includes owners and transitive downstream packages", () => {
  const root = "/repo";
  const core = {
    id: "core 0.1.0",
    name: "core",
    manifest_path: "/repo/crates/core/Cargo.toml",
  };
  const app = {
    id: "app 0.1.0",
    name: "app",
    manifest_path: "/repo/crates/app/Cargo.toml",
  };
  const metadata = {
    packages: [core, app],
    workspace_members: [core.id, app.id],
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
  const sha = "1".repeat(40);
  const deletion = "0".repeat(40);
  const input = [
    `refs/heads/main ${sha} refs/heads/main ${deletion}`,
    `refs/tags/v1 ${sha} refs/tags/v1 ${deletion}`,
    `refs/heads/old ${deletion} refs/heads/old ${sha}`,
  ].join("\n");
  assert.deepEqual(pushedCommits(input, "2".repeat(40)), [sha]);
  assert.deepEqual(pushedCommits("", sha), [sha]);
});

test("pre-push shares only the prepared test environment with exact worktrees", () => {
  const source = readFileSync(
    join(ROOT, "tools", "hooks", "pre-push.mjs"),
    "utf8",
  );
  assert.match(source, /TEST_ENV_CACHE: join\(ROOT, "\.cache", "test-env"\)/);
  assert.doesNotMatch(source, /TEST_ENV_CACHE: join\(ROOT, "\.cache"\)/);
});
