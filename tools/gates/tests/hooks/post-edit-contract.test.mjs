import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs, {
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path, { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import checkEditedFiles, {
  handleToolResult,
} from "../../../../.omp/hooks/post/check-edited-files.js";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");

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
