import { mkdirSync, renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const ARTIFACT_KEYS = new Set(["events", "gate", "outcome", "stage", "trace"]);
const OUTCOME_KEYS = new Set(["code", "kind"]);
const EVENT_KEYS = new Set(["event", "status"]);
const TRACE_KEYS = new Set(["bytes", "format", "records", "summary"]);
const SAFE_WORD = /^[a-z][a-z0-9-]{0,63}$/u;

function assertKeys(value, allowed) {
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) {
      throw new TypeError(`unknown failure artifact key: ${key}`);
    }
  }
}

function assertWord(value, name) {
  if (typeof value !== "string" || !SAFE_WORD.test(value)) {
    throw new TypeError(`${name} must be a safe diagnostic word`);
  }
}

function assertNumber(value, name) {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new TypeError(`${name} must be a non-negative integer`);
  }
}

function validateOutcome(outcome) {
  if (!outcome || typeof outcome !== "object" || Array.isArray(outcome)) {
    throw new TypeError("outcome must be an object");
  }
  assertKeys(outcome, OUTCOME_KEYS);
  assertWord(outcome.kind, "outcome.kind");
  if (Object.hasOwn(outcome, "code")) {
    assertNumber(outcome.code, "outcome.code");
  }
}

function validateEvents(events) {
  if (!Array.isArray(events)) {
    throw new TypeError("events must be an array");
  }
  for (const event of events) {
    if (!event || typeof event !== "object" || Array.isArray(event)) {
      throw new TypeError("event must be an object");
    }
    assertKeys(event, EVENT_KEYS);
    assertWord(event.event, "event.event");
    assertWord(event.status, "event.status");
  }
}

function validateTrace(trace) {
  if (!trace || typeof trace !== "object" || Array.isArray(trace)) {
    throw new TypeError("trace must be an object");
  }
  assertKeys(trace, TRACE_KEYS);
  assertWord(trace.format, "trace.format");
  assertNumber(trace.records, "trace.records");
  assertNumber(trace.bytes, "trace.bytes");
  if (Object.hasOwn(trace, "summary")) {
    assertWord(trace.summary, "trace.summary");
  }
}

function artifactRecord(detail) {
  if (!detail || typeof detail !== "object" || Array.isArray(detail)) {
    throw new TypeError("failure artifact must be an object");
  }
  assertKeys(detail, ARTIFACT_KEYS);
  assertWord(detail.gate, "gate");
  assertWord(detail.stage, "stage");
  validateOutcome(detail.outcome);
  if (Object.hasOwn(detail, "events")) {
    validateEvents(detail.events);
  }
  if (Object.hasOwn(detail, "trace")) {
    validateTrace(detail.trace);
  }
  return { schema: 1, ...detail };
}

export function writeFailureArtifact(directory, detail) {
  const artifact = artifactRecord(detail);
  mkdirSync(directory, { recursive: true });
  const destination = join(directory, `${artifact.gate}-failure.json`);
  const temporary = `${destination}.temporary`;
  writeFileSync(temporary, `${JSON.stringify(artifact, null, 2)}\n`);
  renameSync(temporary, destination);
  return destination;
}

export function recordFailureArtifact(directory, detail) {
  return writeFailureArtifact(directory, detail);
}
