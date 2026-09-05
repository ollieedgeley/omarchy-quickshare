import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs, {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import os, { tmpdir } from "node:os";
import path, { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import checkEditedFiles, {
  handleToolResult,
} from "../../../.omp/hooks/post/check-edited-files.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
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

    const result = spawnSync("node", [RUN_STAGED, "test"], {
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
    const result = spawnSync("node", [RUN_STAGED, "test"], {
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
const FAILURE_RECORD_KEYS = [
  "count",
  "kind",
  "language",
  "rule",
  "timestamp",
  "tool",
];
const FILE_MODE_MODULUS = 0o1000;
const PRIVATE_FILE_MODE = 0o600;

function makePi(overrides = {}) {
  const calls = [];
  return {
    exec(command, args, opts) {
      calls.push({ command, args, opts });
      return { code: 0, stdout: "", stderr: "" };
    },
    calls,
    ...overrides,
  };
}

function makeRealPi() {
  return {
    exec(command, args, opts) {
      const result = spawnSync(command, args, {
        cwd: opts.cwd,
        encoding: "utf8",
        timeout: opts.timeout,
      });
      return {
        code: result.status,
        stderr: result.stderr,
        stdout: result.stdout,
      };
    },
  };
}

function installNodeEditTools(
  directory,
  rules = { quotes: ["error", "double"], semi: ["error", "always"] },
) {
  const nodeModules = join(directory, "node_modules");
  const nodeBin = process.env.NODE_BIN ?? join(ROOT, "node_modules", ".bin");
  mkdirSync(nodeModules);
  fs.symlinkSync(nodeBin, join(nodeModules, ".bin"), "dir");
  const config =
    `export default [{ files: ["**/*.js"], rules: ` +
    `${JSON.stringify(rules)} }];\n`;
  writeFileSync(join(directory, "eslint.config.mjs"), config);
}

function installRuffEditTool(directory) {
  const executable = join(directory, ".cache/tools/ruff-0.16.5/ruff");
  const ruff = process.env.RUFF ?? join(ROOT, ".cache/tools/ruff-0.16.5/ruff");
  mkdirSync(dirname(executable), { recursive: true });
  fs.symlinkSync(ruff, executable);
}

function readPostEditFailures(directory) {
  const log = join(directory, ".cache/omp/post-edit-failures.jsonl");
  return readFileSync(log, "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
}

function omitVerifiedTimestamp(record) {
  const { timestamp, ...withoutTimestamp } = record;
  assert.equal(Number.isNaN(Date.parse(timestamp)), false);
  assert.deepEqual(Object.keys(record).sort(), FAILURE_RECORD_KEYS);
  return withoutTimestamp;
}

function assertPrivateFailureLog(directory) {
  const records = readPostEditFailures(directory);
  const failures = records
    .map(omitVerifiedTimestamp)
    .sort((left, right) => left.tool.localeCompare(right.tool));
  assert.deepEqual(failures, [
    {
      count: 1,
      kind: "lint",
      language: "javascript",
      rule: "no-restricted-syntax",
      tool: "eslint",
    },
    {
      count: 1,
      kind: "lint",
      language: "python",
      rule: "F821",
      tool: "ruff",
    },
  ]);
  const serialized = JSON.stringify(records);
  assert.equal(serialized.includes("PRIVATE_PATH_SENTINEL"), false);
  assert.equal(serialized.includes("PRIVATE_SOURCE_SENTINEL"), false);
  assert.equal(serialized.includes("PRIVATE_MESSAGE_SENTINEL"), false);
  const log = join(directory, ".cache/omp/post-edit-failures.jsonl");
  assert.equal(fs.statSync(log).mode % FILE_MODE_MODULUS, PRIVATE_FILE_MODE);
}

function makeSymlinkEscapeFixture(directory, outside) {
  installNodeEditTools(directory, {
    "no-restricted-syntax": [
      "error",
      {
        message: "PRIVATE_MESSAGE_SENTINEL",
        selector: "CallExpression[callee.name='eval']",
      },
    ],
    quotes: ["error", "double"],
    semi: ["error", "always"],
  });
  const insideFile = join(directory, "inside.js");
  const outsideFile = join(outside, "PRIVATE_OUTSIDE_SENTINEL.js");
  const linkedFile = join(directory, "linked.js");
  writeFileSync(insideFile, "eval('PRIVATE_SOURCE_SENTINEL')\n");
  writeFileSync(outsideFile, "const outside='unchanged'\n");
  fs.symlinkSync(outsideFile, linkedFile);
  fs.symlinkSync(outside, join(directory, ".cache"), "dir");
  return { insideFile, linkedFile, outsideFile };
}

test("default factory registers one handler", () => {
  let registered = 0;
  const pi = {
    on(name) {
      if (name === "tool_result") {
        registered += 1;
      }
    },
  };
  checkEditedFiles(pi);
  assert.equal(registered, 1);
});

test("bypass cases return no replacement result", async () => {
  const pi = makePi();
  const cwd = os.tmpdir();
  const failedEvent = await handleToolResult(
    pi,
    { isError: true, toolName: "write" },
    { cwd },
  );
  assert.equal(typeof failedEvent, "undefined");
  const unrelatedTool = await handleToolResult(
    pi,
    { toolName: "bash" },
    { cwd },
  );
  assert.equal(typeof unrelatedTool, "undefined");
  const internalUri = await handleToolResult(
    pi,
    { toolName: "write", details: { resolvedPath: "xdev:xx" } },
    { cwd },
  );
  assert.equal(typeof internalUri, "undefined");
});

test("post-edit fixes JavaScript and returns success", async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "editfb-"));
  try {
    installNodeEditTools(directory);
    const file = path.join(directory, "sample.js");
    fs.writeFileSync(file, "const greeting='hello'\n");
    const event = {
      toolName: "write",
      details: { resolvedPath: file },
      content: [],
    };

    const output = await handleToolResult(makeRealPi(), event, {
      cwd: directory,
    });

    assert.equal(readFileSync(file, "utf8"), 'const greeting = "hello";\n');
    assert.equal(output.isError, false);
    assert.equal(
      existsSync(join(directory, ".cache/omp/post-edit-failures.jsonl")),
      false,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("post-edit rustfmt changes only the edited Rust file", async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "editfb-"));
  try {
    const root = join(directory, "root.rs");
    const child = join(directory, "child.rs");
    writeFileSync(root, 'mod child;\nfn main(){println!("root");}\n');
    writeFileSync(child, 'pub fn child(){println!("child");}\n');

    const output = await handleToolResult(
      makeRealPi(),
      { details: { resolvedPath: root }, toolName: "write" },
      { cwd: directory },
    );

    assert.equal(
      readFileSync(root, "utf8"),
      'mod child;\nfn main() {\n    println!("root");\n}\n',
    );
    assert.equal(
      readFileSync(child, "utf8"),
      'pub fn child(){println!("child");}\n',
    );
    assert.equal(output.isError, false);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("post-edit reports real unresolved rules privately", async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "editfb-"));
  try {
    installNodeEditTools(directory, {
      "no-restricted-syntax": [
        "error",
        {
          message: "PRIVATE_MESSAGE_SENTINEL",
          selector: "CallExpression[callee.name='eval']",
        },
      ],
    });
    installRuffEditTool(directory);
    const javascript = join(directory, "PRIVATE_PATH_SENTINEL.js");
    const python = join(directory, "private.py");
    writeFileSync(javascript, 'eval("PRIVATE_SOURCE_SENTINEL");\n');
    writeFileSync(python, "print(PRIVATE_SOURCE_SENTINEL)\n");
    const prior = [{ type: "text", text: "prior tool result" }];
    const details = {
      path: javascript,
      perFileResults: [
        { path: javascript },
        { path: python },
        { path: javascript },
      ],
    };

    const output = await handleToolResult(
      makeRealPi(),
      { content: prior, details, toolName: "edit" },
      { cwd: directory },
    );
    assertPrivateFailureLog(directory);

    assert.equal(output.isError, true);
    assert.equal(output.details, details);
    assert.equal(output.content[0], prior[0]);
    assert.equal(output.content.at(-1).text.includes("eslint"), true);
    assert.equal(output.content.at(-1).text.includes("ruff"), true);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("post-edit fails closed and records missing tools", async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "editfb-"));
  try {
    const file = join(directory, "data.json");
    writeFileSync(file, '{"kept":true}\n');

    const output = await handleToolResult(
      makeRealPi(),
      { details: { resolvedPath: file }, toolName: "write" },
      { cwd: directory },
    );
    const records = readPostEditFailures(directory);

    assert.equal(output.isError, true);
    assert.equal(readFileSync(file, "utf8"), '{"kept":true}\n');
    assert.deepEqual(records.map(omitVerifiedTimestamp), [
      {
        count: 1,
        kind: "execution",
        language: "json",
        rule: null,
        tool: "prettier",
      },
    ]);
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
  }
});

test("post-edit rejects target and failure-log symlink escapes", async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "editfb-"));
  const outside = fs.mkdtempSync(path.join(os.tmpdir(), "editfb-outside-"));
  try {
    const { insideFile, linkedFile, outsideFile } = makeSymlinkEscapeFixture(
      directory,
      outside,
    );
    const prior = [{ type: "text", text: "prior tool result" }];

    const output = await handleToolResult(
      makeRealPi(),
      {
        content: prior,
        details: {
          path: insideFile,
          perFileResults: [
            { path: insideFile },
            { path: linkedFile },
            { path: outsideFile },
          ],
        },
        toolName: "edit",
      },
      { cwd: directory },
    );

    assert.equal(output.isError, true);
    assert.equal(output.content[0], prior[0]);
    assert.equal(output.content.at(-1).text.includes("inside.js"), true);
    assert.equal(
      readFileSync(insideFile, "utf8"),
      'eval("PRIVATE_SOURCE_SENTINEL");\n',
    );
    assert.equal(
      readFileSync(outsideFile, "utf8"),
      "const outside='unchanged'\n",
    );
    assert.equal(
      existsSync(join(outside, "omp/post-edit-failures.jsonl")),
      false,
    );
  } finally {
    fs.rmSync(directory, { recursive: true, force: true });
    fs.rmSync(outside, { recursive: true, force: true });
  }
});
