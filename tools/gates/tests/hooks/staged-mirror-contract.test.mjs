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

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const PREPARE = join(ROOT, "tools", "hooks", "prepare-staged.mjs");
const RUN_STAGED = join(ROOT, "tools", "hooks", "run-staged.mjs");
const CODEGRAPH =
  process.env.CODEGRAPH ?? join(ROOT, "node_modules", ".bin", "codegraph");
const EXECUTABLE_MODE = 0o755;
const HOOK_SETUP_PATTERN = /make hooks-install/u;
const PARTIAL_STAGING_PATTERN = /staged and unstaged changes/u;

function childEnvironment(overrides = {}) {
  const environment = { ...process.env, CODEGRAPH, ...overrides };
  delete environment.NODE_TEST_CONTEXT;
  return environment;
}

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
  const executable = join(bin, "eslint");
  const log = join(root, "eslint-argv.json");
  mkdirSync(bin, { recursive: true });
  const eslintScript = [
    "#!/usr/bin/env node",
    'const { readFileSync, writeFileSync } = require("node:fs");',
    "const args = process.argv.slice(2);",
    "const paths = args.filter((arg) => arg.endsWith('.mjs'));",
    `writeFileSync(${JSON.stringify(log)}, JSON.stringify(paths));`,
    "const failed = paths.some((file) =>",
    "  readFileSync(file, 'utf8').includes('forbidden'),",
    ");",
    "process.exitCode = failed ? 1 : 0;",
    "",
  ].join("\n");
  writeFileSync(executable, eslintScript);
  chmodSync(executable, EXECUTABLE_MODE);
  return { bin, log };
}

function fakeAnalyzer(root, name) {
  const bin = join(root, ".fake-analysis-bin");
  const executable = join(bin, name);
  const log = join(root, `${name}-argv.json`);
  const logLiteral = JSON.stringify(log);
  mkdirSync(bin, { recursive: true });
  writeFileSync(
    executable,
    [
      "#!/usr/bin/env node",
      'const { writeFileSync } = require("node:fs");',
      `writeFileSync(${logLiteral}, JSON.stringify(process.argv.slice(2)));`,
      "",
    ].join("\n"),
  );
  chmodSync(executable, EXECUTABLE_MODE);
  return { bin, log };
}

function fakeCodeGraph(root, affectedTests = []) {
  const executable = join(root, "fake-codegraph");
  const log = join(root, "codegraph-argv.json");
  const logLiteral = JSON.stringify(log);
  const codegraphScript = [
    "#!/usr/bin/env node",
    'const { writeFileSync } = require("node:fs");',
    `writeFileSync(${logLiteral}, JSON.stringify(process.argv.slice(2)));`,
    "process.stdout.write(JSON.stringify({",
    `  affectedTests: ${JSON.stringify(affectedTests)},`,
    "  changedFiles: [],",
    "  totalDependentsTraversed: 0,",
    "}));",
    "",
  ].join("\n");
  writeFileSync(executable, codegraphScript);
  chmodSync(executable, EXECUTABLE_MODE);
  return { executable, log };
}

function repository() {
  const root = mkdtempSync(join(tmpdir(), "quickshare-staged-contract-"));
  run("git", ["init", "-b", "main"], root);
  run("git", ["config", "user.name", "Hook Contract"], root);
  run("git", ["config", "user.email", "hook@example.invalid"], root);
  return root;
}
function writeFailingTest(testPath) {
  writeFileSync(
    testPath,
    [
      'import test from "node:test";',
      'test("first fails", () => { throw new Error("expected"); });',
      "",
    ].join("\n"),
  );
}

function writeMarkerTest(testPath, markerPath) {
  writeFileSync(
    testPath,
    [
      'import { writeFileSync } from "node:fs";',
      `writeFileSync(${JSON.stringify(markerPath)}, "ran\\n");`,
      "",
    ].join("\n"),
  );
}

function writeToolAwareMarkerTest(testPath, markerPath) {
  writeFileSync(
    testPath,
    [
      'import { existsSync, writeFileSync } from "node:fs";',
      'if (!existsSync(process.env.AST_GREP ?? "")) {',
      '  throw new Error("AST_GREP executable is missing");',
      "}",
      `writeFileSync(${JSON.stringify(markerPath)}, "ran\\n");`,
      "",
    ].join("\n"),
  );
}

function readSelection(root) {
  return JSON.parse(
    readFileSync(
      join(root, ".cache", "gates", "pre-commit-selection.json"),
      "utf8",
    ),
  );
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

test("staged lint phases receive only their exact paths", () => {
  const root = repository();
  try {
    const eslint = fakeEslint(root);
    const source = join(root, "src.mjs");
    const stagedTest = join(root, "tst.test.mjs");
    writeFileSync(source, "export const staged = true;\n");
    writeFileSync(stagedTest, "// forbidden staged test\n");
    run("git", ["add", source, stagedTest], root);
    writeFileSync(source, "forbidden working tree\n");
    writeFileSync(
      stagedTest,
      "import test from 'node:test'; test('ok', () => {});\n",
    );
    run("node", [PREPARE, "--initialize"], root);

    const sourceResult = spawnSync("node", [RUN_STAGED, "lint-source"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, NODE_BIN: eslint.bin },
    });
    assert.equal(sourceResult.status, 0);
    assert.deepEqual(JSON.parse(readFileSync(eslint.log, "utf8")), ["src.mjs"]);

    const testResult = spawnSync("node", [RUN_STAGED, "lint-tests"], {
      cwd: root,
      encoding: "utf8",
      env: { ...process.env, NODE_BIN: eslint.bin },
    });
    assert.notEqual(testResult.status, 0);
    assert.deepEqual(JSON.parse(readFileSync(eslint.log, "utf8")), [
      "tst.test.mjs",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("analysis phases check exact files then complete touched domains", () => {
  const root = repository();
  try {
    const source = join(root, "tools", "hooks", "change.mjs");
    const domainPeer = join(root, "tools", "gates", "peer.mjs");
    mkdirSync(dirname(source), { recursive: true });
    mkdirSync(dirname(domainPeer), { recursive: true });
    writeFileSync(source, "export const changed = false;\n");
    writeFileSync(domainPeer, "export const peer = true;\n");
    run("git", ["add", source, domainPeer], root);
    run("git", ["commit", "-m", "test: seed analysis fixture"], root);
    writeFileSync(source, "export const changed = true;\n");
    run("git", ["add", source], root);
    run("node", [PREPARE, "--initialize"], root);
    const jscpd = fakeAnalyzer(root, "jscpd");
    const knip = fakeAnalyzer(root, "knip");
    const environment = { ...process.env, NODE_BIN: jscpd.bin };

    runWithOptions("node", [RUN_STAGED, "analysis-source"], {
      cwd: root,
      extraEnvironment: environment,
    });
    assert.deepEqual(JSON.parse(readFileSync(jscpd.log, "utf8")), [
      "--config",
      ".jscpd.json",
      "tools/hooks/change.mjs",
    ]);
    assert.deepEqual(JSON.parse(readFileSync(knip.log, "utf8")), [
      "--strict",
      "--reporter",
      "compact",
    ]);

    runWithOptions("node", [RUN_STAGED, "analysis-domain"], {
      cwd: root,
      extraEnvironment: environment,
    });
    assert.deepEqual(JSON.parse(readFileSync(jscpd.log, "utf8")), [
      "--config",
      ".jscpd.json",
      "tools/gates/peer.mjs",
      "tools/hooks/change.mjs",
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("affected tests receive every staged input and fail fast", () => {
  const root = repository();
  try {
    const source = join(root, "tools", "hooks", "change.mjs");
    const firstTest = join(root, "tools", "gates", "tests", "a.test.mjs");
    const secondTest = join(root, "tools", "gates", "tests", "b.test.mjs");
    const secondTestMarker = join(root, "second-test-ran");
    mkdirSync(dirname(source), { recursive: true });
    mkdirSync(dirname(firstTest), { recursive: true });
    writeFileSync(source, "export const changed = true;\n");
    writeFailingTest(firstTest);
    writeMarkerTest(secondTest, secondTestMarker);
    run("git", ["add", source, firstTest, secondTest], root);
    run("node", [PREPARE, "--initialize"], root);
    const fake = fakeCodeGraph(root);

    const result = spawnSync("node", [RUN_STAGED, "test-tooling"], {
      cwd: root,
      encoding: "utf8",
      env: childEnvironment({ CODEGRAPH: fake.executable }),
    });
    const selectionJson = readFileSync(
      join(root, ".cache", "gates", "pre-commit-selection.json"),
      "utf8",
    );
    assert.notEqual(
      result.status,
      0,
      `${result.stdout}${result.stderr}\n${selectionJson}`,
    );
    assert.equal(existsSync(secondTestMarker), false);
    const codeGraphArguments = JSON.parse(readFileSync(fake.log, "utf8"));
    assert.equal(codeGraphArguments.includes("--filter"), false);
    assert.equal(codeGraphArguments.includes("tools/hooks/change.mjs"), true);
    assert.equal(
      codeGraphArguments.includes("tools/gates/tests/a.test.mjs"),
      true,
    );
    assert.equal(
      codeGraphArguments.includes("tools/gates/tests/b.test.mjs"),
      true,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("QML plugin changes run owned JavaScript tests", () => {
  const root = repository();
  try {
    const pluginTest = join(root, "tools/release/tests/plugin.test.mjs");
    const graphTest = join(root, "tools", "gates", "tests", "graph.test.mjs");
    const source = join(root, "tools", "hooks", "change.mjs");
    const marker = join(root, "plugin-test-ran");
    const nodeBin = join(root, ".fake-bin");
    mkdirSync(dirname(pluginTest), { recursive: true });
    mkdirSync(dirname(graphTest), { recursive: true });
    mkdirSync(dirname(source), { recursive: true });
    mkdirSync(nodeBin);
    writeFileSync(join(nodeBin, "ast-grep"), "");
    writeToolAwareMarkerTest(pluginTest, marker);
    writeFileSync(graphTest, "");
    run("git", ["add", pluginTest, graphTest], root);
    run("git", ["commit", "-m", "test: add plugin contract"], root);

    const qml = join(root, "packaging", "omarchy-plugin", "Panel.qml");
    writeFileSync(source, "export const changed = true;\n");
    mkdirSync(dirname(qml), { recursive: true });
    writeFileSync(qml, "import QtQuick\nItem {}\n");
    run("git", ["add", qml, source], root);
    run("node", [PREPARE, "--initialize"], root);
    const fake = fakeCodeGraph(root, ["tools/gates/tests/graph.test.mjs"]);
    const result = spawnSync("node", [RUN_STAGED, "test-tooling"], {
      cwd: root,
      encoding: "utf8",
      env: childEnvironment({
        CODEGRAPH: fake.executable,
        NODE_BIN: nodeBin,
      }),
    });
    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);

    const selection = readSelection(root);
    assert.equal(existsSync(marker), true, JSON.stringify(selection));
    assert.equal(selection.codegraphCandidates.length, 1);
    assert.equal(
      selection.extendedTests.some(
        (record) => record.path === "tools/release/tests/plugin.test.mjs",
      ),
      true,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

function isolatedGit(root) {
  const extraEnvironment = childEnvironment();
  const local = spawnSync("git", ["rev-parse", "--local-env-vars"], {
    encoding: "utf8",
  });
  assert.equal(local.status, 0, local.stderr);
  for (const name of local.stdout.trim().split("\n")) {
    delete extraEnvironment[name];
  }
  return (args, cwd = root) => {
    const result = spawnSync("git", args, {
      cwd,
      encoding: "utf8",
      env: extraEnvironment,
    });
    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
    return result.stdout.trim();
  };
}

function writeGitFixtureTest(selected, child) {
  mkdirSync(dirname(selected), { recursive: true });
  writeFileSync(
    selected,
    [
      'import assert from "node:assert/strict";',
      'import { execFileSync } from "node:child_process";',
      'import { writeFileSync } from "node:fs";',
      `const cwd = ${JSON.stringify(child)};`,
      'const git = (...args) => execFileSync("git", args, { cwd });',
      'git("init", "-b", "fixture");',
      'git("config", "user.name", "Child Sentinel");',
      'git("config", "user.email", "child@example.invalid");',
      `writeFileSync(${JSON.stringify(join(child, "child.txt"))}, "child");`,
      'git("add", ".");',
      'git("commit", "-m", "test: child");',
      'assert.equal(process.env.GIT_SSH_COMMAND, "ssh -o BatchMode=yes");',
    ].join("\n"),
  );
}

test("staged tooling Git fixtures leave their parent unchanged", () => {
  const root = mkdtempSync(join(tmpdir(), "quickshare-git-isolation-"));
  const git = isolatedGit(root);
  try {
    git(["init", "-b", "main"]);
    git(["config", "user.name", "Parent Sentinel"]);
    git(["config", "user.email", "parent@example.invalid"]);
    git(["commit", "--allow-empty", "-m", "test: parent"]);
    const child = join(root, "child");
    mkdirSync(child);
    const selected = join(root, "tools/gates/tests/git.test.mjs");
    writeGitFixtureTest(selected, child);
    git(["add", "tools"]);
    const snapshot = () => [
      git(["rev-parse", "HEAD"]),
      git(["write-tree"]),
      readFileSync(join(root, ".git/config"), "utf8"),
    ];
    const before = snapshot();
    const env = childEnvironment({
      CODEGRAPH,
      GIT_DIR: join(root, ".git"),
      GIT_INDEX_FILE: join(root, ".git/index"),
      GIT_SSH_COMMAND: "ssh -o BatchMode=yes",
      GIT_WORK_TREE: root,
    });
    const options = { cwd: root, encoding: "utf8", env };
    const prepare = spawnSync("node", [PREPARE, "--initialize"], options);
    assert.equal(prepare.status, 0, `${prepare.stdout}${prepare.stderr}`);
    env.CODEGRAPH = fakeCodeGraph(root).executable;
    const result = spawnSync("node", [RUN_STAGED, "test-tooling"], options);
    assert.deepEqual(snapshot(), before);
    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
    assert.equal(git(["rev-parse", "--show-toplevel"], child), child);
    assert.equal(git(["log", "-1", "--format=%s"], child), "test: child");
    assert.equal(git(["ls-tree", "--name-only", "HEAD"], child), "child.txt");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
