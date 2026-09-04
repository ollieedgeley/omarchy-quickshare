import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test, { describe, it } from "node:test";

import blockMainGates from "../../../.omp/hooks/pre/block-main-gates.js";
import {
  analyzersForPaths,
  duplicationScanPaths,
  selectDomainPaths,
} from "../lib/analysis.mjs";
import {
  codeGraphAffectedArgs,
  computeSelectionRecord,
  getLanguage,
  getRepositoryDomain,
  isCodeGraphIndexableSource,
  isTestPath,
  packageSelection,
  parseAffectedJson,
  selectRustPackages,
} from "../../hooks/affected.mjs";
import { validateCommitMessage } from "../../hooks/commit-msg.mjs";
import { parseNameStatus } from "../../hooks/prepare-staged.mjs";
import { pushedCommits, verificationWorktree } from "../../hooks/pre-push.mjs";
import { parsePackageArgs } from "../rust-lints.mjs";

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
const RUFF_ENV_PATTERN = /RUFF: join\(ROOT, "\.cache", "tools",/u;
const TEST_ENV_PATTERN = /TEST_ENV_CACHE: join\(ROOT, "\.cache", "test-env"\)/u;
const CARGO_MACHETE_ENV_PATTERN =
  /CARGO_MACHETE: join\(\s*ROOT,\s*"\.cache",\s*"tools",/u;
const VULTURE_ENV_PATTERN = /VULTURE: join\(\s*ROOT,\s*"\.cache",\s*"tools",/u;
const BROAD_CACHE_PATTERN = /TEST_ENV_CACHE: join\(ROOT, "\.cache"\)/u;
const DISPOSABLE_WORKTREE_PATTERN = /process\.pid|SHORT_HASH_LENGTH/u;
const PRE_PUSH_LOCK_PATTERN =
  /flock --exclusive \.cache\/gates\/pre-push\.lock/u;
const RUFF_ALL_PATTERN = /select = \["ALL"\]/u;
const RUFF_PREVIEW_PATTERN = /preview = true/u;
const RUFF_VERSION_PATTERN = /RUFF_VERSION="0\.16\.5"/u;
const RUFF_DIGEST_PATTERN = /65b8bae7e43f12a91b71036a52176012/u;
const RUFF_VERIFY_PATTERN = /verify-tooling:.*lint-python/u;
const VERIFY_ORDER_TEST =
  "pre-push verification runs cheap static gates before tests";
const MAKE_CONTINUATION_PATTERN = /\\\n\s*/gu;
const WHITESPACE_PATTERN = /\s+/u;
const AGGREGATE_GATE_PATTERN = /Git hooks own/u;
const OMP_TOOLING_PATTERN = /"\.omp\/"/u;
const FORMAT_SPLIT_TEST =
  "Formatting aggregates keep documentation separate from tooling";
const CODEGRAPH_AFFECTED_TESTS_TEST =
  "CodeGraph candidate parsing uses the documented affectedTests field";
const RUST_LINTS_DEDUPE_TEST =
  "rust-lints parser deduplicates packages and preserves workspace mode";
const PRE_COMMIT_SOURCE_FORMAT_PATTERN = /pre-commit-source-format/u;
const PRE_COMMIT_SOURCE_LINT_PATTERN = /pre-commit-source-lint/u;
const PRE_COMMIT_SOURCE_AST_PATTERN = /pre-commit-source-ast/u;
const PRE_COMMIT_TEST_FORMAT_PATTERN = /pre-commit-test-format/u;
const PRE_COMMIT_TEST_LINT_PATTERN = /pre-commit-test-lint/u;
const PRE_COMMIT_TEST_AST_PATTERN = /pre-commit-test-ast/u;
const PRE_COMMIT_TEST_PATTERN = /pre-commit-test/u;
const PRE_COMMIT_FORMAT_NEGATIVE_PATTERN = /pre-commit-format(?!-source)/u;
const PRE_COMMIT_JAVASCRIPT_PATTERN = /pre-commit-javascript/u;
const PRE_COMMIT_PYTHON_PATTERN = /pre-commit-python/u;
const PRE_COMMIT_AST_NEGATIVE_PATTERN = /pre-commit-ast(?!-)/u;
const PRE_COMMIT_RUST_PATTERN = /pre-commit-rust/u;
const PRE_COMMIT_SOURCE_ANALYSIS_PATTERN = /pre-commit-source-analysis/u;
const PRE_COMMIT_TEST_ANALYSIS_PATTERN = /pre-commit-test-analysis/u;
const PRE_COMMIT_DOMAIN_ANALYSIS_PATTERN = /pre-commit-domain-analysis/u;
const JSCPD_THRESHOLD = 5;
const JSCPD_MINIMUM_LINES = 5;
const JSCPD_MINIMUM_TOKENS = 50;
const CLANG_WARNINGS_ERROR_PATTERN = /WarningsAsErrors:\s*['"]\*['"]/u;
const CARGO_MACHETE_VERSION_PATTERN = /CARGO_MACHETE_VERSION="0\.9\.2"/u;
const VULTURE_VERSION_PATTERN = /VULTURE_VERSION="2\.16"/u;
const CLANG_TIDY_VERSION_PATTERN = /CLANG_TIDY_VERSION="22\.1\.8"/u;
const CPPCHECK_VERSION_PATTERN = /CPPCHECK_VERSION="2\.21\.1"/u;
const CLANG_ANALYZER_PATTERN = /clang-analyzer-\*/u;
const CLANG_BUGPRONE_PATTERN = /bugprone-\*/u;
const CLANG_PERFORMANCE_PATTERN = /performance-\*/u;

function gateGuardResult(command, toolName = "bash") {
  let handler = null;
  blockMainGates({
    on(event, candidate) {
      if (event === "tool_call") {
        handler = candidate;
      }
    },
  });
  assert.equal(typeof handler, "function");
  return handler({ input: { command }, toolName });
}

function cargoPackage({ id, manifestPath, name, targetKinds = ["lib"] }) {
  return Object.fromEntries([
    ["id", id],
    ["name", name],
    ["manifest_path", manifestPath],
    ["targets", targetKinds.map((kind) => ({ kind: [kind] }))],
  ]);
}

function makePrerequisites(source, target) {
  return source
    .replaceAll(MAKE_CONTINUATION_PATTERN, " ")
    .split("\n")
    .filter((line) => line.startsWith(`${target}:`) && !line.includes("##"))
    .join(" ");
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

test(CODEGRAPH_AFFECTED_TESTS_TEST, () => {
  assert.deepEqual(
    parseAffectedJson({
      affectedTests: ["tests/direct.rs", "tests/transitive.rs"],
    }),
    ["tests/direct.rs", "tests/transitive.rs"],
  );
});

test(CARGO_SELECTION_TEST, () => {
  const root = "/repo";
  const core = cargoPackage({
    id: "core 0.1.0",
    manifestPath: "/repo/crates/core/Cargo.toml",
    name: "core",
  });
  const app = cargoPackage({
    id: "app 0.1.0",
    manifestPath: "/repo/crates/app/Cargo.toml",
    name: "app",
  });
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
      { hasLibrary: true, name: "app", root: "crates/app" },
      { hasLibrary: true, name: "core", root: "crates/core" },
    ],
  );
});

test("Cargo selection identifies packages without doc-test targets", () => {
  const root = "/repo";
  const suite = cargoPackage({
    id: "suite 0.0.0",
    manifestPath: "/repo/tests/suites/contracts/Cargo.toml",
    name: "suite",
    targetKinds: ["test"],
  });
  const metadata = {
    packages: [suite],
    resolve: { nodes: [{ id: suite.id, dependencies: [] }] },
    ...Object.fromEntries([["workspace_members", [suite.id]]]),
  };
  assert.deepEqual(
    packageSelection(metadata, ["tests/suites/contracts/tests/suite.rs"], root),
    [
      {
        hasLibrary: false,
        name: "suite",
        root: "tests/suites/contracts",
      },
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

test("pre-push reuses one locked exact-commit worktree", () => {
  const cache = join(ROOT, ".cache", "gates");
  assert.equal(verificationWorktree(cache), join(cache, "pre-push-worktree"));
  const source = readFileSync(
    join(ROOT, "tools", "hooks", "pre-push.mjs"),
    "utf8",
  );
  assert.doesNotMatch(source, DISPOSABLE_WORKTREE_PATTERN);
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  assert.match(makefile, PRE_PUSH_LOCK_PATTERN);
});

test(PRE_PUSH_ENVIRONMENT_TEST, () => {
  const source = readFileSync(
    join(ROOT, "tools", "hooks", "pre-push.mjs"),
    "utf8",
  );
  assert.match(source, AST_GREP_ENV_PATTERN);
  assert.match(source, RUFF_ENV_PATTERN);
  assert.match(source, TEST_ENV_PATTERN);
  assert.match(source, CARGO_MACHETE_ENV_PATTERN);
  assert.match(source, VULTURE_ENV_PATTERN);
  assert.doesNotMatch(source, BROAD_CACHE_PATTERN);
});

test(VERIFY_ORDER_TEST, () => {
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  const prerequisites = makePrerequisites(makefile, "verify");
  const expectedOrder = [
    "format-check",
    "lint-javascript",
    "lint-analysis",
    "check",
    "lint-rust",
    "test-ast-rules",
    "test-rust",
    "test-oracle-toolchain",
  ];
  const gates = prerequisites.split(WHITESPACE_PATTERN);
  const positions = expectedOrder.map((gate) => gates.indexOf(gate));
  assert.ok(positions.every((position) => position >= 0));
  assert.deepEqual(
    positions,
    positions.toSorted((left, right) => left - right),
  );
});

test("analysis aggregate isolates timed analyzer groups", () => {
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  assert.deepEqual(
    makePrerequisites(makefile, "lint-analysis")
      .split(WHITESPACE_PATTERN)
      .slice(1),
    [
      "lint-analysis-general",
      "lint-analysis-clang-tidy",
      "lint-analysis-cppcheck",
    ],
  );
});

test(FORMAT_SPLIT_TEST, () => {
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  assert.deepEqual(
    makePrerequisites(makefile, "format").split(WHITESPACE_PATTERN).slice(1),
    ["format-app", "format-tooling", "format-docs"],
  );
  assert.deepEqual(
    makePrerequisites(makefile, "format-check")
      .split(WHITESPACE_PATTERN)
      .slice(1),
    ["format-app-check", "format-tooling-check", "format-docs-check"],
  );
  assert.ok(
    makefile.includes(
      "TOOLING_FILES = $(REPOSITORY_FILES) -- ':(exclude)*.md'",
    ),
  );
  assert.ok(
    makefile.includes("DOCUMENT_FILES = $(REPOSITORY_FILES) -- '*.md'"),
  );
  assert.ok(
    makefile.includes(
      "verify-tooling: format-tooling-check format-docs-check lint-javascript",
    ),
  );
});

test("Python tooling selects all pinned Ruff rules", () => {
  const config = readFileSync(join(ROOT, "ruff.toml"), "utf8");
  const setup = readFileSync(join(ROOT, "tools", "setup", "ruff.sh"), "utf8");
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  assert.match(config, RUFF_ALL_PATTERN);
  assert.match(config, RUFF_PREVIEW_PATTERN);
  assert.match(setup, RUFF_VERSION_PATTERN);
  assert.match(setup, RUFF_DIGEST_PATTERN);
  assert.match(makefile, RUFF_VERIFY_PATTERN);
  assert.ok(makefile.includes("pre-commit-source-lint"));
  assert.ok(makefile.includes("pre-commit-test-lint"));
});

test("OMP blocks direct aggregate Make gates", () => {
  const commands = [
    "make pre-commit",
    "make pre-push",
    "make verify",
    "make build",
    "CARGO_TARGET_DIR=target/check make -j2 verify",
    "cd /tmp && /usr/bin/make build",
    "timeout 60s make verify",
    "/usr/bin/timeout --foreground 60s make build",
  ];
  for (const command of commands) {
    const result = gateGuardResult(command);
    assert.equal(result?.block, true, command);
    assert.match(result.reason, AGGREGATE_GATE_PATTERN);
  }
});

test("OMP allows narrow gates and Git-owned aggregate gates", () => {
  const commands = [
    "make verify-app",
    "make build-release",
    "make test-rust",
    "git commit -m 'test: exercise hooks'",
    "git push origin main",
    "printf 'make verify\\n'",
  ];
  for (const command of commands) {
    assert.equal(gateGuardResult(command), null, command);
  }
  assert.equal(gateGuardResult("make verify", "eval"), null);
});

test("staged tooling selection includes OMP hooks", () => {
  const source = readFileSync(
    join(ROOT, "tools", "hooks", "affected.mjs"),
    "utf8",
  );
  assert.match(source, OMP_TOOLING_PATTERN);
});

test("rust-lints parser rejects malformed or empty package arguments", () => {
  assert.throws(() => parsePackageArgs(["--package", ""]));
  assert.throws(() => parsePackageArgs(["-p", "--bad"]));
  assert.throws(() => parsePackageArgs(["--package"]));
});

test(RUST_LINTS_DEDUPE_TEST, () => {
  assert.deepEqual(parsePackageArgs([]), []);
  assert.deepEqual(
    parsePackageArgs(["--package", "foo", "-p", "bar", "--package", "foo"]),
    ["foo", "bar"],
  );
});

test("rust-lints parser rejects unknown and positional arguments", () => {
  assert.throws(() => parsePackageArgs(["foo"]));
  assert.throws(() => parsePackageArgs(["--other"]));
  assert.throws(() => parsePackageArgs(["--package", "ok", "extra"]));
});

test("pre-commit uses ordered source and test phase targets", () => {
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  const precommit = makePrerequisites(makefile, "pre-commit");
  assert.match(precommit, PRE_COMMIT_SOURCE_FORMAT_PATTERN);
  assert.match(precommit, PRE_COMMIT_SOURCE_LINT_PATTERN);
  assert.match(precommit, PRE_COMMIT_SOURCE_AST_PATTERN);
  assert.match(precommit, PRE_COMMIT_TEST_FORMAT_PATTERN);
  assert.match(precommit, PRE_COMMIT_TEST_LINT_PATTERN);
  assert.match(precommit, PRE_COMMIT_TEST_AST_PATTERN);
  assert.match(precommit, PRE_COMMIT_SOURCE_ANALYSIS_PATTERN);
  assert.match(precommit, PRE_COMMIT_TEST_ANALYSIS_PATTERN);
  assert.match(precommit, PRE_COMMIT_DOMAIN_ANALYSIS_PATTERN);
  const expectedOrder = [
    "pre-commit-source-format",
    "pre-commit-source-lint",
    "pre-commit-source-ast",
    "pre-commit-source-analysis",
    "pre-commit-test-format",
    "pre-commit-test-lint",
    "pre-commit-test-ast",
    "pre-commit-test-analysis",
    "pre-commit-domain-analysis",
    "pre-commit-test",
  ];
  const gates = precommit.split(WHITESPACE_PATTERN);
  const positions = expectedOrder.map((gate) => gates.indexOf(gate));
  assert.ok(positions.every((position) => position >= 0));
  assert.deepEqual(
    positions,
    positions.toSorted((left, right) => left - right),
  );
  assert.match(precommit, PRE_COMMIT_TEST_PATTERN);
  assert.doesNotMatch(precommit, PRE_COMMIT_FORMAT_NEGATIVE_PATTERN);
  assert.doesNotMatch(precommit, PRE_COMMIT_JAVASCRIPT_PATTERN);
  assert.doesNotMatch(precommit, PRE_COMMIT_PYTHON_PATTERN);
  assert.doesNotMatch(precommit, PRE_COMMIT_AST_NEGATIVE_PATTERN);
  assert.doesNotMatch(precommit, PRE_COMMIT_RUST_PATTERN);
});
describe("codeGraphAffectedArgs", () => {
  it("passes every indexed language in one unfiltered affected query", () => {
    const args = codeGraphAffectedArgs("/mirror", [
      "src/lib.rs",
      "tools/check.mjs",
      "tools/probe.py",
    ]);
    assert.deepStrictEqual(args, [
      "affected",
      "--path",
      "/mirror",
      "--depth",
      "32",
      "--json",
      "src/lib.rs",
      "tools/check.mjs",
      "tools/probe.py",
    ]);
    assert.equal(args.includes("--filter"), false);
  });
});

describe("selectRustPackages", () => {
  it("widens ownerless Rust inputs to all workspace packages", () => {
    const metadata = Object.fromEntries([
      [
        "packages",
        [
          Object.fromEntries([
            ["id", "core"],
            ["manifest_path", "/repo/crates/core/Cargo.toml"],
            ["name", "core"],
            ["targets", [{ kind: ["lib"] }]],
          ]),
          Object.fromEntries([
            ["id", "app"],
            ["manifest_path", "/repo/crates/app/Cargo.toml"],
            ["name", "app"],
            ["targets", [{ kind: ["bin"] }]],
          ]),
        ],
      ],
      ["workspace_members", ["core", "app"]],
    ]);
    assert.deepStrictEqual(
      selectRustPackages(metadata, ["Cargo.toml"], "/repo"),
      [
        { hasLibrary: false, name: "app", root: "crates/app" },
        { hasLibrary: true, name: "core", root: "crates/core" },
      ],
    );
    assert.deepStrictEqual(
      selectRustPackages(metadata, ["README.md"], "/repo"),
      [],
    );
  });
});

describe("isCodeGraphIndexableSource", () => {
  it("recognizes indexed code and excludes generated paths", () => {
    assert.equal(isCodeGraphIndexableSource("src/foo.rs"), true);
    assert.equal(isCodeGraphIndexableSource("tests/bar.test.js"), true);
    assert.equal(isCodeGraphIndexableSource(".omp/tools/hooks/x.mjs"), true);
    assert.equal(isCodeGraphIndexableSource(".hidden"), false);
    assert.equal(isCodeGraphIndexableSource("docs/readme.md"), false);
    assert.equal(isCodeGraphIndexableSource("node_modules/x"), false);
    assert.equal(isCodeGraphIndexableSource(""), false);
  });
});

describe("isTestPath", () => {
  it("recognizes root and nested tests across languages", () => {
    assert.equal(isTestPath("tests/foo.test.js"), true);
    assert.equal(isTestPath("tests/nested/bar.spec.ts"), true);
    assert.equal(isTestPath("src/baz_test.py"), true);
    assert.equal(isTestPath("foo_test.py"), true);
    assert.equal(isTestPath("test_bar.rs"), true);
    assert.equal(isTestPath("tests/foo_test.rs"), true);
    assert.equal(isTestPath("tests/environments/env1/quux_test.py"), true);
    assert.equal(isTestPath("src/main.rs"), false);
    assert.equal(isTestPath("crates/app/src/daemon/media/tests.rs"), true);
    assert.equal(isTestPath("tests/helpers.js"), true);
  });
});

describe("getLanguage", () => {
  it("detects language from every family", () => {
    assert.equal(getLanguage("x.rs"), "rust");
    assert.equal(getLanguage("y_test.py"), "python");
    assert.equal(getLanguage("z.test.js"), "javascript");
    assert.equal(getLanguage("w.sh"), "shell");
    assert.equal(getLanguage("lib.c"), "cpp");
    assert.equal(getLanguage("mod.cpp"), "cpp");
    assert.equal(getLanguage("App.java"), "java");
    assert.equal(getLanguage("Mod.kt"), "kotlin");
    assert.equal(getLanguage("unknown"), "unknown");
    assert.equal(getLanguage("Panel.qml"), "qml");
  });
});
describe("getRepositoryDomain", () => {
  it("classifies repository domains with segment-safe precedence", () => {
    assert.equal(
      getRepositoryDomain("tests/environments/ci/setup.js"),
      "tests/environments/ci",
    );
    assert.equal(
      getRepositoryDomain("tests/suites/unit/foo.rs"),
      "tests/suites/unit",
    );
    assert.equal(getRepositoryDomain("tools/release/foo"), "plugin-release");
    assert.equal(
      getRepositoryDomain("packaging/omarchy-plugin/bar"),
      "plugin-release",
    );
    assert.equal(getRepositoryDomain("tools/oracle/q.rs"), "oracle");
    assert.equal(getRepositoryDomain("tools/oracle/manifest.json"), "oracle");
    assert.equal(getRepositoryDomain("crates/app/main.rs"), "crates/app");
    assert.equal(
      getRepositoryDomain("crates/core/net/src/lib.rs"),
      "crates/core/net",
    );
    assert.equal(
      getRepositoryDomain(".omp/tools/hooks/affected.mjs"),
      "tooling",
    );
    assert.equal(getRepositoryDomain("tools/hooks/affected.mjs"), "tooling");
    assert.equal(getRepositoryDomain("biome.json"), "tooling");
    assert.equal(getRepositoryDomain("nested/config.json"), "other");
    assert.equal(getRepositoryDomain("nested/package.json"), "other");
    assert.equal(getRepositoryDomain("src/mytarget/foo.rs"), "other");
    assert.equal(getRepositoryDomain("Cargo.toml"), "cargo-workspace");
    assert.equal(getRepositoryDomain("package.json"), "tooling");
    assert.equal(getRepositoryDomain("README.md"), "docs");
    assert.equal(getRepositoryDomain("other.rs"), "other");
  });
});

describe("computeSelectionRecord", () => {
  it("separates staged sources, staged tests, and extended tests", () => {
    const staged = [
      "src/main.rs",
      "tests/a.test.js",
      "b_test.py",
      "tests/a.test.js",
      "src/main.rs",
      "README.md",
    ];
    const affected = ["tests/a.test.js", "c.spec.ts", "d_test.py", "src/e.rs"];
    const rec = computeSelectionRecord(staged, affected);
    assert.deepStrictEqual(rec.stagedSources, [
      { path: "README.md", language: "unknown", domain: "docs" },
      { path: "src/main.rs", language: "rust", domain: "other" },
    ]);
    assert.deepStrictEqual(rec.stagedTests, [
      { path: "b_test.py", language: "python", domain: "other" },
      { path: "tests/a.test.js", language: "javascript", domain: "other" },
    ]);
    assert.deepStrictEqual(rec.extendedTests, [
      { path: "c.spec.ts", language: "javascript", domain: "other" },
      { path: "d_test.py", language: "python", domain: "other" },
    ]);
  });
});

test("analysis dispatches every strict supported tool", () => {
  assert.deepEqual(
    analyzersForPaths([
      "Cargo.toml",
      "src/lib.rs",
      "tools/check.mjs",
      "tools/check.py",
      "tools/check.cc",
      "README.md",
    ]),
    [
      "jscpd",
      "cargo-machete",
      "knip",
      "ruff",
      "vulture",
      "clang-tidy",
      "cppcheck",
    ],
  );
  assert.deepEqual(analyzersForPaths(["package.json"]), ["jscpd", "knip"]);
  assert.deepEqual(analyzersForPaths(["tools/check.cc"], ["clang-tidy"]), [
    "clang-tidy",
  ]);
  assert.deepEqual(analyzersForPaths(["tools/check.cc"], ["cppcheck"]), [
    "cppcheck",
  ]);
});

test("test-only analysis skips jscpd on a tests-only corpus", () => {
  assert.deepEqual(
    duplicationScanPaths(
      [
        "crates/app/tests/daemon_process.rs",
        "tools/release/tests/plugin-release-contract.test.mjs",
      ],
      "files",
    ),
    [],
  );
  assert.deepEqual(duplicationScanPaths(["crates/app/src/lib.rs"], "files"), [
    "crates/app/src/lib.rs",
  ]);
  assert.deepEqual(duplicationScanPaths(["src/lib.rs"], "full"), ["."]);
});

test("domain analysis widens only to staged domains", () => {
  assert.deepEqual(
    selectDomainPaths(
      ["tools/hooks/run-staged.mjs", "crates/core/wire/src/lib.rs"],
      [
        "crates/app/src/main.rs",
        "crates/core/wire/src/lib.rs",
        "crates/core/wire/tests/wire.rs",
        "tools/gates/structure.mjs",
        "tools/hooks/run-staged.mjs",
        "tools/release/plugin-export.mjs",
      ],
    ),
    [
      "crates/core/wire/src/lib.rs",
      "crates/core/wire/tests/wire.rs",
      "tools/gates/structure.mjs",
      "tools/hooks/run-staged.mjs",
    ],
  );
});

test("analysis configuration keeps findings fatal at production limits", () => {
  const duplication = JSON.parse(
    readFileSync(join(ROOT, ".jscpd.json"), "utf8"),
  );
  const knip = JSON.parse(readFileSync(join(ROOT, "knip.json"), "utf8"));
  const clangTidy = readFileSync(join(ROOT, ".clang-tidy"), "utf8");
  const packageManifest = JSON.parse(
    readFileSync(join(ROOT, "package.json"), "utf8"),
  );
  const setup = readFileSync(
    join(ROOT, "tools", "setup", "analyzers.sh"),
    "utf8",
  );
  assert.equal(duplication.threshold, JSCPD_THRESHOLD);
  assert.equal(duplication.minLines, JSCPD_MINIMUM_LINES);
  assert.equal(duplication.minTokens, JSCPD_MINIMUM_TOKENS);
  assert.deepEqual(duplication.reporters, ["console", "threshold"]);
  assert.equal(knip.includeEntryExports, true);
  assert.equal(
    Object.values(knip.rules).every((severity) => severity === "error"),
    true,
  );
  assert.equal(packageManifest.devDependencies.jscpd, "5.1.2");
  assert.equal(packageManifest.devDependencies.knip, "6.34.0");
  assert.match(setup, CARGO_MACHETE_VERSION_PATTERN);
  assert.match(setup, VULTURE_VERSION_PATTERN);
  assert.match(setup, CLANG_TIDY_VERSION_PATTERN);
  assert.match(setup, CPPCHECK_VERSION_PATTERN);
  assert.match(clangTidy, CLANG_WARNINGS_ERROR_PATTERN);
  assert.match(clangTidy, CLANG_ANALYZER_PATTERN);
  assert.match(clangTidy, CLANG_BUGPRONE_PATTERN);
  assert.match(clangTidy, CLANG_PERFORMANCE_PATTERN);
});
