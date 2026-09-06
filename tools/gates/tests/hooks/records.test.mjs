import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import {
  createReport,
  runRecorded,
  updateReport,
} from "../../../hooks/records.mjs";

const REVISION = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PRIVATE_FILE_MODE = 0o600;
const PRIVATE_DIRECTORY_MODE = 0o700;
const MODE_RANGE = 0o1000;
const LARGE_OUTPUT_BYTES = 2097152;
const CHILD_FAILURE = 7;

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), "gate-records-"));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const reportPath = createReport({
    kind: "pre-commit",
    revision: REVISION,
    root,
    selection: { gates: ["test-rust-lan"] },
  });
  return {
    read: () => JSON.parse(readFileSync(reportPath, "utf8")),
    reportPath,
    root,
    run(script, env = process.env) {
      return runRecorded({
        args: ["--input-type=module", "-e", script],
        command: process.execPath,
        cwd: root,
        env,
        id: "test",
        kind: "gate",
        reasons: [{ kind: "staged", path: "crates/app/src/lib.rs" }],
        reportPath,
      });
    },
  };
}

test("records retain started state and private output", (context) => {
  const repo = fixture(context);
  const script = [
    'import assert from "node:assert/strict";',
    'import { readFileSync, writeSync } from "node:fs";',
    `const reportPath = ${JSON.stringify(repo.reportPath)};`,
    'const record = JSON.parse(readFileSync(reportPath, "utf8"));',
    'assert.equal(record.status, "started");',
    'assert.equal(record.steps[0].status, "started");',
    "assert.equal(record.steps[0].cwd, process.cwd());",
    `writeSync(1, "x".repeat(${LARGE_OUTPUT_BYTES}));`,
    'writeSync(2, "stderr-end");',
  ].join("\n");
  const result = repo.run(script, {
    ...process.env,
    PRIVATE_TOKEN: "not-output",
  });
  assert.equal(result.status, 0);
  const report = repo.read();
  assert.equal(report.revision, REVISION);
  assert.equal(report.steps[0].status, "passed");
  assert.equal(report.status, "started");
  const [{ logPath }] = report.steps;
  assert.equal(
    readFileSync(logPath, "utf8"),
    `${"x".repeat(LARGE_OUTPUT_BYTES)}stderr-end`,
  );
  assert.equal(statSync(logPath).mode % MODE_RANGE, PRIVATE_FILE_MODE);
  assert.equal(statSync(repo.reportPath).mode % MODE_RANGE, PRIVATE_FILE_MODE);
  assert.equal(
    statSync(dirname(repo.reportPath)).mode % MODE_RANGE,
    PRIVATE_DIRECTORY_MODE,
  );
  assert.ok(!readFileSync(repo.reportPath, "utf8").includes("not-output"));
  updateReport(repo.reportPath, { status: "passed" });
  assert.deepEqual(repo.read().selection, { gates: ["test-rust-lan"] });
});

test("failure attribution uses the first actual Make diagnostic", (context) => {
  const repo = fixture(context);
  const cases = [
    [
      "make[2]: *** [tools/gates/rust.mk:24: test-rust-lan] Error 1\n" +
        "make: *** [Makefile:4: verify] Error 2",
      "test-rust-lan",
    ],
    ["gmake: *** [child-check] Error 7", "child-check"],
    ["make[1]: *** [Makefile:10: test-signal] Terminated", "test-signal"],
    ["application: failed target hypothetical\n", null],
  ];
  for (const [output, target] of cases) {
    assert.throws(() =>
      repo.run(`console.error(${JSON.stringify(output)}); process.exit(7);`),
    );
    const step = repo.read().steps.at(-1);
    assert.equal(step.status, "failed");
    assert.equal(step.exitCode, CHILD_FAILURE);
    assert.equal(step.failedMakeTarget, target);
    assert.equal(readFileSync(step.logPath, "utf8"), `${output}\n`);
  }
});

test("spawn failures persist without invented targets", (context) => {
  const repo = fixture(context);
  assert.throws(() =>
    runRecorded({
      args: [],
      command: join(repo.root, "missing-command"),
      cwd: repo.root,
      id: "missing",
      kind: "gate",
      reportPath: repo.reportPath,
    }),
  );
  const [step] = repo.read().steps;
  assert.equal(step.status, "failed");
  assert.ok(step.error.includes("ENOENT"));
  assert.equal(Object.hasOwn(step, "failedMakeTarget"), false);
  assert.equal(readFileSync(step.logPath, "utf8"), "");
});
