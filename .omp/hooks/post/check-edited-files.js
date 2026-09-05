import fs from "node:fs";
import path from "node:path";

import { pipelineForFile } from "../support/edited-file-pipeline.js";

const BOUND = 2048;
const TIMEOUT_MS = 60000;
const PRIVATE_DIRECTORY_MODE = 0o700;
const PRIVATE_FILE_MODE = 0o600;
const INTERNAL_URI_RE = /^[a-z][a-z0-9+.-]*:/iu;
const MARKDOWN_RULE_RE = /\bMD\d{3}\b/gu;

function bounded(input) {
  const text = String(input || "");
  if (text.length > BOUND) {
    return `${text.slice(0, BOUND)}\n... (truncated)`;
  }
  return text;
}

function extractCandidates(event) {
  const candidates = new Set();
  if (event.toolName === "write") {
    const candidate =
      event.details?.resolvedPath || event.details?.path || event.input?.path;
    if (candidate) {
      candidates.add(candidate);
    }
  } else if (event.toolName === "edit") {
    if (event.details?.path) {
      candidates.add(event.details.path);
    }
    for (const result of event.details?.perFileResults || []) {
      if (result?.path) {
        candidates.add(result.path);
      }
    }
  }
  return [...candidates];
}

function contained(base, candidate) {
  const relative = path.relative(base, candidate);
  return (
    relative !== ".." &&
    !relative.startsWith("../") &&
    !path.isAbsolute(relative)
  );
}

function addValidTarget(targets, base, candidate) {
  let lexical = candidate;
  if (!path.isAbsolute(candidate)) {
    lexical = path.resolve(base, candidate);
  }
  if (!contained(base, lexical)) {
    return;
  }
  try {
    const real = fs.realpathSync(lexical);
    if (contained(base, real) && fs.statSync(real).isFile()) {
      targets.add(real);
    }
  } catch {
    // A failed, missing, or non-file tool target is not an edited file.
  }
}

function resolveValidTargets(candidates, cwd) {
  const base = fs.realpathSync(path.resolve(cwd || process.cwd()));
  const targets = new Set();
  for (const candidate of candidates) {
    if (typeof candidate === "string" && !INTERNAL_URI_RE.test(candidate)) {
      addValidTarget(targets, base, candidate);
    }
  }
  return { base, targets: [...targets] };
}

function errorMessage(error) {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function executionFailure(step, error, result) {
  const output = `${result?.stdout ?? ""}\n${result?.stderr ?? ""}`.trim();
  let diagnostic = `${step.name} could not run: ${errorMessage(error)}`;
  if (output) {
    diagnostic += `\n${output}`;
  }
  return {
    diagnostic: bounded(diagnostic),
    kind: "execution",
    rule: null,
    step,
  };
}

async function execute(pi, step, cwd) {
  try {
    const result = await pi.exec(step.command, step.args, {
      cwd,
      timeout: TIMEOUT_MS,
    });
    if (!result || !Number.isInteger(result.code)) {
      return executionFailure(step, "tool returned no exit status");
    }
    return { result, step };
  } catch (error) {
    return executionFailure(step, error);
  }
}

function machineMessages(name, output) {
  const parsed = JSON.parse(output);
  if (name !== "eslint") {
    return parsed;
  }
  return parsed.flatMap((entry) => entry.messages || []);
}

function machineRule(message) {
  if (message.ruleId !== null && typeof message.ruleId !== "undefined") {
    return message.ruleId;
  }
  return message.code ?? null;
}

function machineDiagnostic(name, message, file) {
  const location = message.location?.row ?? message.line;
  const column = message.location?.column ?? message.column;
  const rule = machineRule(message);
  let ruleText = "";
  if (rule) {
    ruleText = ` [${rule}]`;
  }
  const place = `${file}:${location ?? "?"}:${column ?? "?"}`;
  return `${place} ${name}${ruleText} ${message.message}`;
}

function parseMachineLint(name, output, file) {
  try {
    const messages = machineMessages(name, output);
    if (!Array.isArray(messages) || messages.length === 0) {
      return null;
    }
    return {
      diagnostic: messages
        .map((message) => machineDiagnostic(name, message, file))
        .join("\n"),
      rules: messages.map(machineRule),
    };
  } catch {
    return null;
  }
}

function lintDetails(step, output, file) {
  if (step.name === "markdownlint-cli2") {
    const rules = output.match(MARKDOWN_RULE_RE);
    if (rules?.length) {
      return { diagnostic: output, rules };
    }
    return null;
  }
  return parseMachineLint(step.name, output, file);
}

function unresolvedFailure(step, result, file) {
  const output = `${result.stdout ?? ""}\n${result.stderr ?? ""}`.trim();
  if (step.kind === "format") {
    return {
      diagnostic: bounded(
        `${step.name} found unresolved format errors:\n${output}`,
      ),
      kind: "format",
      rule: null,
      step,
    };
  }
  let lintOutput = result.stdout ?? "";
  if (step.name === "markdownlint-cli2") {
    lintOutput = output;
  }
  const details = lintDetails(step, lintOutput, file);
  if (!details) {
    return {
      diagnostic: bounded(
        `${step.name} returned unreadable lint output:\n${output}`,
      ),
      kind: "unclassified",
      rule: null,
      step,
    };
  }
  return {
    diagnostic: bounded(
      `${step.name} found unresolved lint errors:\n${details.diagnostic}`,
    ),
    kind: "lint",
    rules: details.rules,
    step,
  };
}

function recordsForFailure(failure, language, timestamp) {
  const rules = failure.rules ?? [failure.rule];
  const counts = new Map();
  for (const rule of rules) {
    counts.set(rule, (counts.get(rule) ?? 0) + 1);
  }
  return [...counts].map(([rule, count]) => ({
    count,
    kind: failure.kind,
    language,
    rule,
    timestamp,
    tool: failure.step.name,
  }));
}

function secureDirectory(directory, base) {
  if (!fs.existsSync(directory)) {
    fs.mkdirSync(directory, { mode: PRIVATE_DIRECTORY_MODE });
  }
  const status = fs.lstatSync(directory);
  const real = fs.realpathSync(directory);
  if (
    !status.isDirectory() ||
    status.isSymbolicLink() ||
    !contained(base, real)
  ) {
    throw new Error(`refusing unsafe failure-log directory: ${directory}`);
  }
}

function appendFailureRecords(base, records) {
  const cache = path.join(base, ".cache");
  const directory = path.join(cache, "omp");
  secureDirectory(cache, base);
  secureDirectory(directory, base);
  const log = path.join(directory, "post-edit-failures.jsonl");
  const flags =
    fs.constants.O_APPEND +
    fs.constants.O_CREAT +
    fs.constants.O_WRONLY +
    fs.constants.O_NOFOLLOW;
  const descriptor = fs.openSync(log, flags, PRIVATE_FILE_MODE);
  try {
    const status = fs.fstatSync(descriptor);
    if (!status.isFile()) {
      throw new Error("failure log is not a regular file");
    }
    fs.fchmodSync(descriptor, PRIVATE_FILE_MODE);
    const batch = `${records
      .map((record) => JSON.stringify(record))
      .join("\n")}\n`;
    fs.writeSync(descriptor, batch);
  } finally {
    fs.closeSync(descriptor);
  }
}

function rereadNotice(files, base) {
  let subject = "these files";
  if (files.length === 1) {
    subject = "this file";
  }
  const paths = files.map((file) => path.relative(base, file)).join(", ");
  const instruction =
    `Re-read ${subject} before any anchored edit; post-edit autofix made ` +
    "previous snapshot anchors stale.";
  return `${instruction} Changed: ${paths}`;
}

function recordOutcome(state, step, outcome) {
  if (outcome.kind === "execution") {
    state.failures.push(outcome);
    state.brokenTools.add(step.name);
  } else if (outcome.result.code > 1 && step.kind !== "format") {
    state.failures.push(
      executionFailure(
        step,
        `tool exited with status ${outcome.result.code}`,
        outcome.result,
      ),
    );
    state.brokenTools.add(step.name);
  } else if (!state.fix && outcome.result.code !== 0) {
    state.failures.push(unresolvedFailure(step, outcome.result, state.file));
  }
}

async function runSteps(state) {
  if (state.index >= state.steps.length) {
    return;
  }
  const step = state.steps[state.index];
  state.index += 1;
  if (!state.fix && state.brokenTools.has(step.name)) {
    await runSteps(state);
    return;
  }
  const outcome = await execute(state.pi, step, state.base);
  recordOutcome(state, step, outcome);
  await runSteps(state);
}

async function processFile(pi, file, base) {
  const pipeline = pipelineForFile(file, base);
  if (!pipeline) {
    return { changed: false, failures: [], language: null };
  }
  const before = fs.readFileSync(file);
  const state = {
    base,
    brokenTools: new Set(),
    failures: [],
    file: path.relative(base, file),
    fix: true,
    index: 0,
    pi,
  };
  state.steps = pipeline.fixes;
  await runSteps(state);
  state.fix = false;
  state.index = 0;
  state.steps = pipeline.checks;
  await runSteps(state);
  return {
    changed: !before.equals(fs.readFileSync(file)),
    failures: state.failures,
    language: pipeline.language,
  };
}

async function processTargets(state) {
  if (state.index >= state.targets.length) {
    return;
  }
  const file = state.targets[state.index];
  state.index += 1;
  const result = await processFile(state.pi, file, state.base);
  if (result.changed) {
    state.changed.push(file);
  }
  state.failures.push(...result.failures);
  for (const failure of result.failures) {
    const records = recordsForFailure(
      failure,
      result.language,
      state.timestamp,
    );
    state.records.push(...records);
  }
  await processTargets(state);
}

function rereadIfChanged(changed, base) {
  if (changed.length) {
    return rereadNotice(changed, base);
  }
  return "";
}

function replacement(event, text, isError) {
  return {
    content: [...(event.content || []), { type: "text", text: bounded(text) }],
    details: event.details,
    isError,
  };
}

function finalResult(event, state) {
  const notice = rereadIfChanged(state.changed, state.base);
  if (state.failures.length) {
    const diagnostics = state.failures
      .map((failure) => failure.diagnostic)
      .join("\n\n");
    const text = [notice, diagnostics].filter(Boolean).join("\n\n");
    return replacement(event, text, true);
  }
  if (notice) {
    return replacement(event, notice, false);
  }
  return globalThis.undefined;
}

export async function handleToolResult(pi, event, ctx) {
  if (
    event.isError ||
    (event.toolName !== "write" && event.toolName !== "edit")
  ) {
    return globalThis.undefined;
  }
  const state = {
    base: "",
    changed: [],
    failures: [],
    index: 0,
    pi,
    records: [],
    targets: [],
    timestamp: new Date().toISOString(),
  };
  try {
    const resolved = resolveValidTargets(extractCandidates(event), ctx?.cwd);
    state.base = resolved.base;
    state.targets = resolved.targets;
    await processTargets(state);
    if (state.records.length) {
      appendFailureRecords(state.base, state.records);
    }
    return finalResult(event, state);
  } catch (error) {
    const notice = rereadIfChanged(state.changed, state.base);
    const diagnostic = `Post-edit hook failed closed: ${errorMessage(error)}`;
    const text = [notice, diagnostic].filter(Boolean).join("\n\n");
    return replacement(event, text, true);
  }
}

export default function checkEditedFiles(pi) {
  pi.on("tool_result", (event, ctx) => handleToolResult(pi, event, ctx));
}
