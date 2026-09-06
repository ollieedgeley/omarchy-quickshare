import {
  closeSync,
  fsyncSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { randomUUID } from "node:crypto";

import { run } from "../gates/lib/process.mjs";

const PRIVATE_FILE_MODE = 0o600;
const PRIVATE_DIRECTORY_MODE = 0o700;
const REVISION = /^[a-f\d]{40}(?:[a-f\d]{24})?$/u;
const MAKE_FAILURE = new RegExp(
  String.raw`^\s*g?make(?:\[\d+\])?: \*\*\* ` +
    String.raw`\[(?:[^\]\n]*:\d+: )?(?<target>[^\]\n]+)\]`,
  "mu",
);

export function updateReport(reportPath, patch) {
  const record = { ...JSON.parse(readFileSync(reportPath, "utf8")), ...patch };
  const temporary = `${reportPath}.${randomUUID()}.tmp`;
  const descriptor = openSync(temporary, "wx", PRIVATE_FILE_MODE);
  try {
    writeFileSync(descriptor, `${JSON.stringify(record, null, 2)}\n`);
    fsyncSync(descriptor);
  } finally {
    closeSync(descriptor);
  }
  renameSync(temporary, reportPath);
}

export function createReport({ root, kind, revision, selection }) {
  if (!["pre-commit", "pre-push"].includes(kind) || !REVISION.test(revision)) {
    throw new Error(
      `invalid gate report kind or revision: ${kind}/${revision}`,
    );
  }
  const parent = resolve(root, ".cache", "gates", "runs", kind, revision);
  mkdirSync(parent, { recursive: true, mode: PRIVATE_DIRECTORY_MODE });
  const attempt = mkdtempSync(join(parent, "attempt-"));
  const reportPath = join(attempt, "report.json");
  writeFileSync(reportPath, "{}\n", { mode: PRIVATE_FILE_MODE, flag: "wx" });
  updateReport(reportPath, {
    kind,
    revision,
    selection,
    startedAt: new Date().toISOString(),
    status: "started",
    steps: [],
  });
  process.stdout.write(`Gate report: ${reportPath}\n`);
  return reportPath;
}

function persistStep(reportPath, step) {
  const report = JSON.parse(readFileSync(reportPath, "utf8"));
  const steps = report.steps.filter((entry) => entry.logPath !== step.logPath);
  updateReport(reportPath, { steps: [...steps, step] });
}

function failedMakeTarget(logPath) {
  const match = MAKE_FAILURE.exec(readFileSync(logPath, "utf8"));
  return match?.groups.target ?? null;
}

function executeStep(options, step) {
  const result = run(options.command, options.args, {
    allowFailure: true,
    cwd: step.cwd,
    env: options.env,
    logPath: step.logPath,
    quiet: true,
  });
  step.exitCode = result.status;
  step.signal = result.signal;
  if (result.status !== 0) {
    step.failedMakeTarget = failedMakeTarget(step.logPath);
    const rendered = [options.command, ...options.args].join(" ");
    throw new Error(`${rendered} exited with ${result.status}`);
  }
  return result;
}

export function runRecorded(options) {
  const { reportPath, id, kind, command, args, reasons } = options;
  const step = {
    args,
    command,
    cwd: resolve(options.cwd),
    durationMs: 0,
    id,
    kind,
    logPath: join(dirname(reportPath), `${randomUUID()}.log`),
    reasons,
    startedAt: new Date().toISOString(),
    status: "started",
  };
  persistStep(reportPath, step);
  process.stdout.write(
    `+ ${command} ${args.join(" ")}\n  log: ${step.logPath}\n`,
  );
  const started = performance.now();
  try {
    const result = executeStep(options, step);
    step.status = "passed";
    return result;
  } catch (error) {
    step.status = "failed";
    step.error = error.message;
    throw error;
  } finally {
    if (step.failedMakeTarget) {
      process.stderr.write(`Failed Make target: ${step.failedMakeTarget}\n`);
    }
    step.durationMs = performance.now() - started;
    persistStep(reportPath, step);
    process.stdout.write(`${id}: ${step.status}; log: ${step.logPath}\n`);
  }
}
