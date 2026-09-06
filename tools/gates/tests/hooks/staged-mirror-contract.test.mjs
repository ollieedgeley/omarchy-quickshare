import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
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

function readReport(root) {
  const metadata = JSON.parse(
    readFileSync(join(root, ".cache/gates/staged.json"), "utf8"),
  );
  return JSON.parse(readFileSync(metadata.reportPath, "utf8"));
}

function readSelection(root) {
  return readReport(root).selection;
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
    const selectionJson = JSON.stringify(readSelection(root));
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

function fakeMake(root, scenario) {
  const bin = join(root, ".fake-make");
  const log = join(root, "make-calls.jsonl");
  mkdirSync(bin);
  const executable = join(bin, "make");
  writeFileSync(
    executable,
    [
      "#!/usr/bin/env node",
      'const { appendFileSync, readFileSync } = require("node:fs");',
      `const scenario = ${JSON.stringify(scenario)};`,
      `const log = ${JSON.stringify(log)};`,
      "const record = { args: process.argv.slice(2), cwd: process.cwd(),",
      "  source: readFileSync(scenario, 'utf8') };",
      "appendFileSync(log, JSON.stringify(record) + '\\n');",
      "if (record.args.includes(process.env.FAIL_MAKE_TARGET)) {",
      "  process.stderr.write(",
      '    "make[1]: *** [Makefile:42: child-proof] Error 1\\n");',
      "  process.exitCode = 1;",
      "}",
    ].join("\n"),
  );
  chmodSync(executable, EXECUTABLE_MODE);
  return { bin, log };
}

function readMakeCalls(log) {
  if (!existsSync(log)) {
    return [];
  }
  return readFileSync(log, "utf8").trim().split("\n").map(JSON.parse);
}

test("staged e2e runs its prepared Make gate from index bytes", () => {
  const root = repository();
  try {
    const scenario =
      "tests/environments/diverse-lan/rust/scenarios/rust-lan-inbound.e2e.mjs";
    const rawMarker = join(root, "raw-e2e-ran");
    mkdirSync(dirname(join(root, scenario)), { recursive: true });
    writeMarkerTest(join(root, scenario), rawMarker);
    run("git", ["add", scenario], root);
    run("node", [PREPARE, "--initialize"], root);
    writeFileSync(join(root, scenario), "throw new Error('dirty checkout');\n");
    symlinkSync(join(ROOT, "tools"), join(root, "tools"), "dir");
    const { bin, log } = fakeMake(root, scenario);
    const result = spawnSync(
      "/usr/bin/make",
      ["-f", join(ROOT, "tools/gates/hooks.mk"), "pre-commit-test"],
      {
        cwd: root,
        encoding: "utf8",
        env: childEnvironment({
          CODEGRAPH: fakeCodeGraph(root).executable,
          PATH: `${bin}:${process.env.PATH}`,
        }),
      },
    );
    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
    const calls = readMakeCalls(log);
    assert.ok(
      calls.some((call) => call.args.includes("test-rust-lan-inbound")),
      `staged executable test was not run: ${JSON.stringify(calls)}`,
    );
    const provision = calls.findIndex((call) =>
      call.args.includes("rust-lan-provision"),
    );
    const child = calls.findIndex((call) =>
      call.args.includes("test-rust-lan-inbound"),
    );
    assert.ok(provision >= 0 && provision < child);
    assert.ok(
      calls.every(
        (call) =>
          call.cwd === join(root, ".cache/gates/pre-commit-tree") &&
          !call.source.includes("dirty checkout"),
      ),
    );
    assert.equal(existsSync(rawMarker), false);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

function runIntegrationFixture(root, path, options = {}) {
  const { bin, log } = fakeMake(root, path);
  const result = spawnSync("node", [RUN_STAGED, "test-integrations"], {
    cwd: root,
    encoding: "utf8",
    env: childEnvironment({
      CODEGRAPH: fakeCodeGraph(root, options.affectedTests ?? []).executable,
      FAIL_MAKE_TARGET: options.failTarget ?? "",
      PATH: `${bin}:${process.env.PATH}`,
    }),
  });
  return { calls: readMakeCalls(log), report: readReport(root), result };
}

test("graph-selected e2e runs for an unrelated staged source", () => {
  const root = repository();
  try {
    const path =
      "tests/environments/diverse-lan/rust/scenarios/rust-lan-outbound.e2e.mjs";
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(
      join(root, path),
      "throw new Error('raw node is forbidden');",
    );
    const secondPath = path.replace("outbound", "inbound");
    writeFileSync(join(root, secondPath), "// second graph-selected scenario");
    run("git", ["add", path, secondPath], root);
    run("git", ["commit", "-m", "test: seed scenario"], root);
    writeFileSync(join(root, "source.mjs"), "export const value = true;");
    run("git", ["add", "source.mjs"], root);
    run("node", [PREPARE, "--initialize"], root);
    const { result, calls } = runIntegrationFixture(root, path, {
      affectedTests: [path, secondPath],
    });
    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`);
    assert.ok(
      calls.some((call) => call.args.includes("test-rust-lan-outbound")),
    );
    assert.equal(
      calls.filter((call) => call.args.includes("rust-lan-provision")).length,
      1,
    );
    assert.equal(
      calls.filter((call) => call.args.includes("nearby-linux-provision"))
        .length,
      1,
    );
    assert.equal(
      calls.filter((call) =>
        call.args.some((arg) => arg.startsWith("test-rust-lan-")),
      ).length,
      2,
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("failed integration cleans up and retains its failing child log", () => {
  const root = repository();
  try {
    const path = "tests/environments/oracle/selected-gtest.mjs";
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), "// staged oracle input");
    run("git", ["add", path], root);
    run("node", [PREPARE, "--initialize"], root);
    const { result, calls, report } = runIntegrationFixture(root, path, {
      failTarget: "test-oracle-ble",
    });
    assert.notEqual(result.status, 0);
    const targets = calls.flatMap((call) => call.args);
    assert.equal(
      targets.filter((item) => item === "oracle-reference-up").length,
      1,
    );
    assert.equal(targets.at(-1), "oracle-reference-down");
    assert.equal(targets.filter((item) => item.startsWith("test-")).length, 1);
    assert.equal(report.status, "failed");
    const failure = report.steps.find((step) => step.status === "failed");
    assert.equal(failure.failedMakeTarget, "child-proof");
    assert.ok(readFileSync(failure.logPath, "utf8").includes("child-proof"));
    assert.ok(
      report.steps.some(
        (step) => step.kind === "cleanup" && step.status === "passed",
      ),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("unmapped executables fail closed with revision evidence", () => {
  const root = repository();
  try {
    const path = "tests/unmapped.e2e.mjs";
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), "// cannot silently skip this executable");
    run("git", ["add", path], root);
    run("node", [PREPARE, "--initialize"], root);
    const { result, calls, report } = runIntegrationFixture(root, path);
    assert.notEqual(result.status, 0);
    assert.deepEqual(calls, []);
    assert.equal(report.status, "failed");
    assert.ok(report.failure.message.includes(path));
    assert.equal(
      report.revision,
      run("git", ["write-tree"], root).stdout.trim(),
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("changed mirror blocks execution and attempt history survives", () => {
  const root = repository();
  try {
    writeFileSync(join(root, "source.mjs"), "export const value = true;");
    run("git", ["add", "source.mjs"], root);
    run("node", [PREPARE, "--initialize"], root);
    const metadataPath = join(root, ".cache/gates/staged.json");
    const first = JSON.parse(readFileSync(metadataPath, "utf8"));
    writeFileSync(join(first.mirror, "source.mjs"), "// tampered snapshot");
    const { result, calls, report } = runIntegrationFixture(root, "source.mjs");
    assert.notEqual(result.status, 0);
    assert.deepEqual(calls, []);
    assert.equal(report.status, "failed");
    const saved = readFileSync(first.reportPath, "utf8");
    run("node", [PREPARE], root);
    const next = JSON.parse(readFileSync(metadataPath, "utf8"));
    assert.notEqual(next.reportPath, first.reportPath);
    assert.equal(next.tree, first.tree);
    assert.equal(readFileSync(first.reportPath, "utf8"), saved);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("mirror mode changes cannot reuse unchanged blob bytes", () => {
  const root = repository();
  try {
    const source = join(root, "source.mjs");
    writeFileSync(source, "export const value = true;");
    run("git", ["add", source], root);
    const mutations = [
      (file) => chmodSync(file, EXECUTABLE_MODE),
      (file) => {
        rmSync(file);
        symlinkSync(source, file);
      },
    ];
    for (const mutate of mutations) {
      run("node", [PREPARE, "--initialize"], root);
      const metadata = JSON.parse(
        readFileSync(join(root, ".cache/gates/staged.json"), "utf8"),
      );
      mutate(join(metadata.mirror, "source.mjs"));
      const result = spawnSync("node", [RUN_STAGED, "test-integrations"], {
        cwd: root,
        encoding: "utf8",
        env: childEnvironment(),
      });
      assert.notEqual(result.status, 0);
      assert.equal(readReport(root).status, "failed");
      assert.deepEqual(readReport(root).steps, []);
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
