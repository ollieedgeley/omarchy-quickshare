import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SECRET = "private-gate-output-must-not-be-recorded";
const TOOLING_SELECTION = /tools\/hooks\/pre-push-timing\.test\.mjs/u;
const PARENT_GIT_VARIABLES = [
  "GIT_ALTERNATE_OBJECT_DIRECTORIES",
  "GIT_COMMON_DIR",
  "GIT_DIR",
  "GIT_INDEX_FILE",
  "GIT_OBJECT_DIRECTORY",
  "GIT_PREFIX",
  "GIT_WORK_TREE",
];

function isolatedGitEnv(extra = {}) {
  const env = { ...process.env, ...extra };
  for (const name of PARENT_GIT_VARIABLES) {
    delete env[name];
  }
  return env;
}

function prepareRepository(root) {
  for (const file of [
    "tools/hooks/pre-push.mjs",
    "tools/gates/lib/process.mjs",
  ]) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    copyFileSync(join(ROOT, file), join(root, file));
  }
  writeFileSync(
    join(root, "Makefile"),
    ["verify build:", `\t@${process.execPath} fixture.cjs $@`, ""].join("\n"),
  );
  writeFileSync(
    join(root, "fixture.cjs"),
    [
      'const fs = require("node:fs");',
      "const gate = process.argv[2];",
      'fs.appendFileSync(process.env.CALL_LOG, gate + "\\n");',
      "if (process.env.FAIL_GATE === gate) {",
      "  console.error(process.env.PRIVATE_TOKEN); process.exit(1);",
      "}",
    ].join("\n"),
  );
  const git = (args) => {
    const result = spawnSync("git", args, {
      cwd: root,
      encoding: "utf8",
      env: isolatedGitEnv(),
    });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git(["init", "--quiet"]);
  git(["add", "."]);
  git([
    "-c",
    "user.name=Hook Test",
    "-c",
    "user.email=hook@example.invalid",
    "-c",
    "core.hooksPath=/dev/null",
    "commit",
    "--quiet",
    "-m",
    "test: timing fixture",
  ]);
  return git(["rev-parse", "HEAD"]);
}

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), "pre-push-timing-"));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const sha = prepareRepository(root);
  const log = join(root, "calls.log");
  return {
    invoke(failGate = "") {
      return spawnSync(
        process.execPath,
        [join(root, "tools/hooks/pre-push.mjs")],
        {
          cwd: root,
          encoding: "utf8",
          env: isolatedGitEnv({
            CALL_LOG: log,
            FAIL_GATE: failGate,
            PRIVATE_TOKEN: SECRET,
          }),
          input: "",
        },
      );
    },
    log,
    record() {
      const text = readFileSync(
        join(root, ".cache", "gates", `pre-push-${sha}.json`),
        "utf8",
      );
      assert.ok(!text.includes(SECRET));
      return JSON.parse(text);
    },
    root,
    sha,
  };
}

function assertRecord(record, sha, { names, status }) {
  assert.equal(record.sha, sha);
  assert.equal(record.status, status);
  assert.deepEqual(
    record.gates.map((gate) => gate.name),
    names,
  );
  for (const gate of record.gates) {
    assert.ok(Number.isFinite(gate.durationMs) && gate.durationMs >= 0);
  }
}

test("pre-push records HEAD timings without reusing results", (context) => {
  const repo = fixture(context);
  const first = repo.invoke();
  assert.equal(first.status, 0, first.stderr);
  const record = repo.record();
  assertRecord(record, repo.sha, {
    names: ["make verify", "make build"],
    status: "passed",
  });
  assert.deepEqual(
    record.gates.map((gate) => gate.status),
    ["passed", "passed"],
  );
  assert.deepEqual(record.artifacts, ["target"]);
  for (const artifact of record.artifacts) {
    assert.equal(isAbsolute(artifact), false);
    assert.ok(
      !relative(repo.root, resolve(repo.root, artifact)).startsWith(".."),
    );
  }
  const second = repo.invoke();
  assert.equal(second.status, 0, second.stderr);
  assert.equal(
    readFileSync(repo.log, "utf8"),
    "verify\nbuild\nverify\nbuild\n",
  );
});

test("pre-push records verify failure and skips build", (context) => {
  const repo = fixture(context);
  const result = repo.invoke("verify");
  assert.notEqual(result.status, 0);
  assert.equal(readFileSync(repo.log, "utf8"), "verify\n");
  const record = repo.record();
  assertRecord(record, repo.sha, {
    names: ["make verify"],
    status: "failed",
  });
  assert.equal(record.gates[0].status, "failed");
  assert.deepEqual(record.artifacts, []);
});

test("pre-push replaces success with build failure", (context) => {
  const repo = fixture(context);
  assert.equal(repo.invoke().status, 0);
  assert.equal(repo.record().status, "passed");
  const result = repo.invoke("build");
  assert.notEqual(result.status, 0);
  const record = repo.record();
  assertRecord(record, repo.sha, {
    names: ["make verify", "make build"],
    status: "failed",
  });
  assert.deepEqual(
    record.gates.map((gate) => gate.status),
    ["passed", "failed"],
  );
  assert.deepEqual(record.artifacts, []);
  assert.equal(
    readFileSync(repo.log, "utf8"),
    "verify\nbuild\nverify\nbuild\n",
  );
});

test("tooling tests select the pre-push timing contracts", () => {
  const packageManifest = JSON.parse(
    readFileSync(join(ROOT, "package.json"), "utf8"),
  );
  assert.match(packageManifest.scripts["test:tooling"], TOOLING_SELECTION);
});
