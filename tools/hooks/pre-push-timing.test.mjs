import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const SECRET = "private-gate-output-must-not-be-recorded";
const UNIX_SOCKET_PATH_LIMIT = 107;
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

function fixtureGit(root, args) {
  const result = spawnSync("git", args, {
    cwd: root,
    encoding: "utf8",
    env: isolatedGitEnv(),
  });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function prepareRepository(root, script, makefile) {
  for (const file of [
    "tools/hooks/pre-push.mjs",
    "tools/hooks/records.mjs",
    "tools/gates/lib/process.mjs",
  ]) {
    mkdirSync(dirname(join(root, file)), { recursive: true });
    copyFileSync(join(ROOT, file), join(root, file));
  }
  writeFileSync(join(root, "Makefile"), makefile);
  writeFileSync(
    join(root, "fixture.cjs"),
    [
      'const fs = require("node:fs");',
      "const gate = process.argv[2];",
      'fs.appendFileSync(process.env.CALL_LOG, gate + "\\n");',
      "if (process.env.OBSERVATION_LOG) {",
      '  const { execFileSync } = require("node:child_process");',
      "  const git = (...args) =>",
      '    execFileSync("git", args, { encoding: "utf8" }).trim();',
      "  fs.appendFileSync(process.env.OBSERVATION_LOG, JSON.stringify({",
      '    gate, root: git("rev-parse", "--show-toplevel"),',
      '    sha: git("rev-parse", "HEAD"),',
      "    ssh: process.env.GIT_SSH_COMMAND,",
      '  }) + "\\n");',
      "}",
      "if (process.env.FAIL_GATE === gate) {",
      "  console.error(process.env.PRIVATE_TOKEN); process.exit(1);",
      `}\n${script}`,
    ].join("\n"),
  );
  const git = (args) => fixtureGit(root, args);
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

function readReports(root, sha) {
  const parent = join(root, ".cache", "gates", "runs", "pre-push", sha);
  return readdirSync(parent).map((attempt) => {
    const text = readFileSync(join(parent, attempt, "report.json"), "utf8");
    assert.ok(!text.includes(SECRET));
    return JSON.parse(text);
  });
}
function fixture(context, { directory = "", script = "", makefile } = {}) {
  const base = mkdtempSync(join(tmpdir(), "pre-push-timing-"));
  context.after(() => rmSync(base, { recursive: true, force: true }));
  const root = join(base, directory);
  mkdirSync(root, { recursive: true });
  const defaultMakefile = [
    "verify build:",
    `\t@${process.execPath} fixture.cjs $@`,
    "",
  ].join("\n");
  const sha = prepareRepository(root, script, makefile ?? defaultMakefile);
  const deletion = "0".repeat(sha.length);
  const update = `refs/heads/main ${sha} refs/heads/main ${deletion}\n`;
  const log = join(root, "calls.log");
  return {
    invoke(failGate = "", input = update, env = {}) {
      return spawnSync(
        process.execPath,
        [join(root, "tools/hooks/pre-push.mjs")],
        {
          cwd: root,
          encoding: "utf8",
          env: {
            ...isolatedGitEnv({
              CALL_LOG: log,
              FAIL_GATE: failGate,
              PRIVATE_TOKEN: SECRET,
            }),
            ...env,
          },
          input,
        },
      );
    },
    log,
    record() {
      const reports = readReports(root, sha);
      return reports.toSorted((left, right) =>
        right.startedAt.localeCompare(left.startedAt),
      )[0];
    },
    root,
    sha,
  };
}

function assertRecord(record, sha, { names, status }) {
  assert.equal(record.revision, sha);
  assert.equal(record.status, status);
  assert.deepEqual(
    record.steps.map((gate) => [gate.command, ...gate.args].join(" ")),
    names,
  );
  for (const gate of record.steps) {
    assert.ok(Number.isFinite(gate.durationMs) && gate.durationMs >= 0);
  }
}

test("pre-push skips gates and worktrees without ref updates", (context) => {
  const repo = fixture(context);
  const result = repo.invoke("", "");
  assert.equal(result.status, 0, result.stderr);
  assert.equal(existsSync(repo.log), false);
  assert.equal(
    existsSync(join(repo.root, ".cache", "gates", "pre-push-worktree")),
    false,
  );
});

test("pre-push records tip timings without reusing results", (context) => {
  const repo = fixture(context);
  const first = repo.invoke();
  assert.equal(first.status, 0, first.stderr);
  const record = repo.record();
  assertRecord(record, repo.sha, {
    names: ["make verify", "make build"],
    status: "passed",
  });
  assert.deepEqual(
    record.steps.map((gate) => gate.status),
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

test("pre-push retains retries and nested failures", (context) => {
  const repo = fixture(context, {
    makefile: [
      "verify:",
      "\t@$(MAKE) --no-print-directory child-check",
      "child-check:",
      `\t@${process.execPath} fixture.cjs verify`,
      "build:",
      `\t@${process.execPath} fixture.cjs build`,
      "",
    ].join("\n"),
  });
  assert.equal(repo.invoke().status, 0);
  const failed = repo.invoke("verify");
  assert.notEqual(failed.status, 0);
  assert.equal(readFileSync(repo.log, "utf8"), "verify\nbuild\nverify\n");
  const attempts = join(
    repo.root,
    ".cache",
    "gates",
    "runs",
    "pre-push",
    repo.sha,
  );
  assert.ok(existsSync(attempts), "revision-keyed attempt history must exist");
  const reports = readdirSync(attempts).map((attempt) =>
    JSON.parse(readFileSync(join(attempts, attempt, "report.json"), "utf8")),
  );
  assert.equal(
    reports.length,
    2,
    "a failed retry must not erase prior evidence",
  );
  assert.deepEqual(reports.map((report) => report.status).sort(), [
    "failed",
    "passed",
  ]);
  const failure = reports.find((report) => report.status === "failed");
  assert.equal(failure.revision, repo.sha);
  assert.equal(failure.steps.length, 1);
  assert.equal(failure.steps[0].failedMakeTarget, "child-check");
  assert.equal(
    failure.steps[0].cwd,
    join(repo.root, ".cache", "gates", "pre-push-worktree"),
  );
  assert.ok(
    readFileSync(failure.steps[0].logPath, "utf8").includes("child-check"),
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
  assert.equal(record.steps[0].status, "failed");
  assert.deepEqual(record.artifacts, []);
});

test("pre-push records build failure after success", (context) => {
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
    record.steps.map((gate) => gate.status),
    ["passed", "failed"],
  );
  assert.deepEqual(record.artifacts, []);
  assert.equal(
    readFileSync(repo.log, "utf8"),
    "verify\nbuild\nverify\nbuild\n",
  );
});

function dirtyParent(repo) {
  const git = (...args) => fixtureGit(repo.root, args);
  writeFileSync(join(repo.root, "parent.txt"), "parent commit\n");
  git("add", "parent.txt");
  git(
    "-c",
    "user.name=Hook Test",
    "-c",
    "user.email=hook@example.invalid",
    "-c",
    "core.hooksPath=/dev/null",
    "commit",
    "--quiet",
    "-m",
    "test: parent checkout",
  );
  writeFileSync(join(repo.root, "parent.txt"), "staged parent\n");
  git("add", "parent.txt");
  writeFileSync(join(repo.root, "parent.txt"), "unstaged parent\n");
  writeFileSync(join(repo.root, "untracked.txt"), "keep untracked\n");
}

test("pre-push isolates verification from caller Git metadata", (context) => {
  const repo = fixture(context);
  const git = (...args) => fixtureGit(repo.root, args);
  dirtyParent(repo);
  const parentFiles = [
    ".git/HEAD",
    ".git/index",
    ".git/config",
    "parent.txt",
    "untracked.txt",
  ];
  const before = parentFiles.map((file) => readFileSync(join(repo.root, file)));
  const parentRef = git("symbolic-ref", "HEAD");
  const parentTip = git("rev-parse", parentRef);
  const observations = join(repo.root, "observations.jsonl");
  const worktree = join(repo.root, ".cache", "gates", "pre-push-worktree");
  const env = {
    GIT_COMMON_DIR: join(repo.root, ".git"),
    GIT_DIR: join(repo.root, ".git"),
    GIT_INDEX_FILE: join(repo.root, ".git", "index"),
    GIT_PREFIX: "",
    GIT_SSH_COMMAND: "ssh -o BatchMode=yes",
    GIT_WORK_TREE: repo.root,
    OBSERVATION_LOG: observations,
  };
  const deletion = "0".repeat(repo.sha.length);
  const input = `refs/heads/main ${repo.sha} refs/heads/main ${deletion}\n`;
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const result = repo.invoke("", input, env);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(git("rev-parse", parentRef), parentTip);
    assert.deepEqual(
      parentFiles.map((file) => readFileSync(join(repo.root, file))),
      before,
    );
  }
  assert.deepEqual(
    readFileSync(observations, "utf8").trim().split("\n").map(JSON.parse),
    ["verify", "build", "verify", "build"].map((gate) => ({
      gate,
      root: worktree,
      sha: repo.sha,
      ssh: env.GIT_SSH_COMMAND,
    })),
  );
});

test("pre-push uses a short cache target for Unix sockets", (context) => {
  const cache = mkdtempSync(join(tmpdir(), "pre-push-socket-"));
  context.after(() => rmSync(cache, { recursive: true, force: true }));
  const repo = fixture(context, {
    directory: "w".repeat(UNIX_SOCKET_PATH_LIMIT),
    script: [
      'const assert = require("node:assert/strict");',
      'const net = require("node:net");',
      'const path = require("node:path");',
      'const socketPath = path.join(process.env.TEST_ENV_CACHE, "gate.sock");',
      "const timer = setTimeout(() => process.exit(1), 3000);",
      'const server = net.createServer(socket => socket.end("reachable"));',
      "server.listen(socketPath, () => {",
      "  assert.ok(fs.existsSync(socketPath));",
      "  const client = net.createConnection(socketPath);",
      '  let reply = "";',
      '  client.setEncoding("utf8");',
      '  client.on("data", data => { reply += data; });',
      '  client.on("end", () => {',
      '    assert.equal(reply, "reachable");',
      "    server.close(() => clearTimeout(timer));",
      "  });",
      "});",
    ].join("\n"),
  });
  const alias = join(repo.root, ".cache", "test-env");
  mkdirSync(dirname(alias), { recursive: true });
  symlinkSync(cache, alias);
  assert.ok(
    Buffer.byteLength(join(alias, "gate.sock")) > UNIX_SOCKET_PATH_LIMIT,
  );
  const result = repo.invoke();
  assert.equal(result.status, 0, result.stderr);
  assert.equal(readFileSync(repo.log, "utf8"), "verify\nbuild\n");
});
