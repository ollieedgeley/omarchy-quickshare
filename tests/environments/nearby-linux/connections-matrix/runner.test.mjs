import assert from "node:assert/strict";
import { test } from "vitest";

import { matrixCases, runConnectionsMatrix } from "./runner.mjs";

const DIGEST_LENGTH = 64;
const PAYLOAD_BYTES = 11;
const SELECTED_PAIR = "ble-to-bluetooth";
const SHA256 = "a".repeat(DIGEST_LENGTH);
const START_TIME = 10;
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

test("selects one pair and rejects invalid or unknown selectors", () => {
  const selected = matrixCases(SELECTED_PAIR);

  assert(selected.every(({ pair }) => pair === SELECTED_PAIR));
  assert.throws(() => matrixCases(""), INVALID_PAIR_ERROR);
  assert.throws(() => matrixCases("ble-to-unknown"), UNKNOWN_PAIR_ERROR);
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

  assert.deepEqual(
    cleared,
    Array.from({ length: nextTimer }, (_value, index) => index + 1),
  );
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
