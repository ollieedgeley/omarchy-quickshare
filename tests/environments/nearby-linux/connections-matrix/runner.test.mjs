import assert from "node:assert/strict";
import test from "node:test";

import { matrixCases, runConnectionsMatrix } from "./runner.mjs";

const CASE_COUNT = 35;
const DIGEST_LENGTH = 64;
const OUTCOME_COUNT = 7;
const PAIR_COUNT = 5;
const PAYLOAD_BYTES = 11;
const SELECTED_PAIR = "ble-to-bluetooth";
const SHA256 = "a".repeat(DIGEST_LENGTH);
const START_TIME = 10;
const INTEGRITY_CASE =
  "accepts only executed, integrity-checked, cleaned-up evidence";
const TIMEOUT_MS = 20;
const TIMEOUT_CASE =
  "fails closed when a route runs past its deterministic deadline";
const TIMEOUT_ERROR = /exceeded its time limit/u;
const UNEXECUTED_ERROR = /did not execute/u;
const UNKNOWN_PAIR_ERROR = /pair is unknown/u;
const INVALID_PAIR_ERROR = /pair selector is invalid/u;

const PAYLOAD = { bytes: PAYLOAD_BYTES, sha256: SHA256 };

function passingEvidence(scenario) {
  return {
    cleanup: { channels: true, fixtures: true, peers: true },
    diagnostic: { kind: "observed", stage: "terminal" },
    executed: true,
    initial: scenario.initial,
    outcome: scenario.outcome,
    payload: PAYLOAD,
    proposer: scenario.proposer,
    terminal: scenario.terminal,
    upgrade: scenario.upgrade,
  };
}

test("expands five two-peer pairs into thirty-five observable cases", () => {
  const cases = matrixCases();

  assert.equal(cases.length, CASE_COUNT);
  assert.deepEqual(
    new Set(cases.map(({ pair }) => pair)),
    new Set([
      "ble-to-bluetooth",
      "ble-to-wifi-lan",
      "ble-to-wifi-hotspot",
      "bluetooth-to-wifi-lan",
      "bluetooth-to-wifi-hotspot",
    ]),
  );
  assert.deepEqual(
    new Set(cases.map(({ outcome }) => outcome)),
    new Set([
      "success",
      "rejection",
      "candidate-disappears",
      "new-channel-loss",
      "simultaneous-proposals",
      "old-channel-fallback",
      "cancellation",
    ]),
  );
  assert(cases.some(({ proposer }) => proposer === "peer-a"));
  assert(cases.some(({ proposer }) => proposer === "peer-b"));
  assert(cases.some(({ proposer }) => proposer === "both"));
  assert(cases.some(({ accepter }) => accepter === "peer-a"));
  assert(cases.some(({ accepter }) => accepter === "peer-b"));
});

test("selects one validated pair without dropping any required outcome", () => {
  const selected = matrixCases(SELECTED_PAIR);

  assert.equal(selected.length, OUTCOME_COUNT);
  assert(selected.every(({ pair }) => pair === SELECTED_PAIR));
  assert.equal(
    new Set(selected.map(({ outcome }) => outcome)).size,
    OUTCOME_COUNT,
  );
  assert.throws(() => matrixCases(""), INVALID_PAIR_ERROR);
  assert.throws(() => matrixCases("ble-to-unknown"), UNKNOWN_PAIR_ERROR);
});

test(INTEGRITY_CASE, async () => {
  const report = await runConnectionsMatrix({
    adapter: {
      execute: (scenario) => Promise.resolve(passingEvidence(scenario)),
    },
    context: { now: () => START_TIME, timeoutMs: TIMEOUT_MS },
    payload: PAYLOAD,
  });

  assert.equal(report.cases.length, CASE_COUNT);
  assert.deepEqual(report.counts, {
    cases: CASE_COUNT,
    outcomes: OUTCOME_COUNT,
    pairs: PAIR_COUNT,
  });
  assert.equal(report.schema, 1);
});

test("reports pair and outcome counts for a selected child gate", async () => {
  const report = await runConnectionsMatrix({
    adapter: {
      execute: (scenario) => Promise.resolve(passingEvidence(scenario)),
    },
    context: { now: () => START_TIME, timeoutMs: TIMEOUT_MS },
    pair: SELECTED_PAIR,
    payload: PAYLOAD,
  });

  assert.equal(report.cases.length, OUTCOME_COUNT);
  assert.deepEqual(report.counts, {
    cases: OUTCOME_COUNT,
    outcomes: OUTCOME_COUNT,
    pairs: 1,
  });
});

test("clears every success timeout handle", async () => {
  const cleared = [];
  let nextTimer = 0;
  await runConnectionsMatrix({
    adapter: {
      execute: (scenario) => Promise.resolve(passingEvidence(scenario)),
    },
    context: {
      cancel: (timer) => cleared.push(timer),
      now: () => START_TIME,
      schedule: () => {
        nextTimer += 1;
        return nextTimer;
      },
      timeoutMs: TIMEOUT_MS,
    },
    payload: PAYLOAD,
  });

  assert.equal(cleared.length, CASE_COUNT);
  assert.equal(cleared.at(-1), CASE_COUNT);
});

test("fails closed when an adapter reports an unexecuted route", async () => {
  await assert.rejects(
    runConnectionsMatrix({
      adapter: { execute: () => Promise.resolve({ executed: false }) },
      context: { now: () => START_TIME, timeoutMs: TIMEOUT_MS },
      payload: PAYLOAD,
    }),
    UNEXECUTED_ERROR,
  );
});

test(TIMEOUT_CASE, async () => {
  const never = new Promise((resolve) => {
    setTimeout(resolve, TIMEOUT_MS);
  });
  await assert.rejects(
    runConnectionsMatrix({
      adapter: { execute: () => never },
      context: {
        cancel: (timer) => assert.equal(timer, "expired"),
        now: () => START_TIME,
        schedule: (callback) => {
          callback();
          return "expired";
        },
        timeoutMs: TIMEOUT_MS,
      },
      payload: PAYLOAD,
    }),
    TIMEOUT_ERROR,
  );
});
