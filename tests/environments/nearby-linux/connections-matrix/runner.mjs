import { readFileSync } from "node:fs";

const MANIFEST = new URL("./manifest.json", import.meta.url);
const DIGEST = /^[0-9a-f]{64}$/u;
const MAX_TIMEOUT_MS = 60_000;
const MEDIUM = /^[a-z][a-z0-9_]*$/u;
const OUTCOME_COUNT = 7;
const PAIR_COUNT = 5;
const SAFE_WORD = /^[a-z][a-z0-9-]*$/u;
const SCHEMA = 1;

function manifest() {
  return JSON.parse(readFileSync(MANIFEST, "utf8"));
}

function validPair(pair, ids) {
  return (
    pair &&
    SAFE_WORD.test(pair.id) &&
    MEDIUM.test(pair.initial) &&
    MEDIUM.test(pair.upgrade) &&
    pair.initial !== pair.upgrade &&
    !ids.has(pair.id)
  );
}

function validOutcome(outcome, outcomes) {
  return (
    outcome &&
    SAFE_WORD.test(outcome.id) &&
    ["peer-a", "peer-b", "both"].includes(outcome.proposer) &&
    SAFE_WORD.test(outcome.terminal) &&
    !outcomes.has(outcome.id)
  );
}

function assertPairs(pairs) {
  const ids = new Set();
  for (const pair of pairs) {
    if (!validPair(pair, ids)) {
      throw new Error("Connections matrix has an invalid medium pair");
    }
    ids.add(pair.id);
  }
  if (!ids.has("ble-to-bluetooth")) {
    throw new Error("Connections matrix lacks BLE to Bluetooth coverage");
  }
}

function assertOutcomes(list) {
  const outcomes = new Set();
  for (const outcome of list) {
    if (!validOutcome(outcome, outcomes)) {
      throw new Error("Connections matrix has an invalid outcome");
    }
    outcomes.add(outcome.id);
  }
  if (!outcomes.has("old-channel-fallback")) {
    throw new Error("Connections matrix lacks old-channel fallback");
  }
}

function assertManifest(value) {
  if (
    value?.schema !== SCHEMA ||
    !Array.isArray(value.pairs) ||
    value.pairs.length !== PAIR_COUNT ||
    !Array.isArray(value.outcomes) ||
    value.outcomes.length !== OUTCOME_COUNT
  ) {
    throw new Error("Connections matrix manifest is incomplete");
  }
  assertPairs(value.pairs);
  assertOutcomes(value.outcomes);
}

function accepter(proposer) {
  if (proposer === "both") {
    return "both";
  }
  if (proposer === "peer-a") {
    return "peer-b";
  }
  return "peer-a";
}

function selectPair(cases, pair = null) {
  if (pair === null) {
    return cases;
  }
  if (typeof pair !== "string" || !SAFE_WORD.test(pair)) {
    throw new Error("Connections matrix pair selector is invalid");
  }
  const selected = cases.filter((scenario) => scenario.pair === pair);
  if (selected.length === 0) {
    throw new Error(`Connections matrix pair is unknown: ${pair}`);
  }
  return selected;
}

export function matrixCases(pair) {
  const value = manifest();
  assertManifest(value);
  const cases = value.pairs.flatMap((mediumPair) =>
    value.outcomes.map((outcome) => ({
      accepter: accepter(outcome.proposer),
      id: `${mediumPair.id}-${outcome.id}`,
      initial: mediumPair.initial,
      outcome: outcome.id,
      pair: mediumPair.id,
      proposer: outcome.proposer,
      terminal: outcome.terminal,
      upgrade: mediumPair.upgrade,
    })),
  );
  return selectPair(cases, pair);
}

function timing(context = {}) {
  const now = context.now ?? Date.now;
  const cancel = context.cancel ?? clearTimeout;
  const schedule = context.schedule ?? setTimeout;
  if (
    typeof now !== "function" ||
    typeof cancel !== "function" ||
    typeof schedule !== "function" ||
    !Number.isInteger(context.timeoutMs) ||
    context.timeoutMs < 1 ||
    context.timeoutMs > MAX_TIMEOUT_MS
  ) {
    throw new Error("Connections matrix needs a bounded timing context");
  }
  return { cancel, now, schedule, timeoutMs: context.timeoutMs };
}

function assertPayload(payload) {
  if (
    !payload ||
    !Number.isInteger(payload.bytes) ||
    payload.bytes < 0 ||
    typeof payload.sha256 !== "string" ||
    !DIGEST.test(payload.sha256)
  ) {
    throw new Error("Connections matrix needs an exact payload digest");
  }
}

function assertEvidenceShape(evidence, scenario) {
  const allowed = new Set([
    "cleanup",
    "diagnostic",
    "executed",
    "initial",
    "outcome",
    "payload",
    "proposer",
    "terminal",
    "upgrade",
  ]);
  if (!evidence || Object.keys(evidence).some((key) => !allowed.has(key))) {
    throw new Error(`${scenario.id} returned unsafe diagnostics`);
  }
}

function assertRouteEvidence(evidence, scenario) {
  if (evidence.executed !== true) {
    throw new Error(`${scenario.id} did not execute its requested route`);
  }
  for (const key of ["initial", "outcome", "proposer", "terminal", "upgrade"]) {
    if (evidence[key] !== scenario[key]) {
      throw new Error(`${scenario.id} reported different ${key}`);
    }
  }
}

function assertCleanup(evidence, scenario) {
  const { cleanup } = evidence;
  if (!cleanup || !cleanup.channels || !cleanup.fixtures || !cleanup.peers) {
    throw new Error(`${scenario.id} did not clean up`);
  }
}

function assertDiagnostic(evidence, scenario) {
  const { diagnostic } = evidence;
  if (
    !diagnostic ||
    !SAFE_WORD.test(diagnostic.kind ?? "") ||
    !SAFE_WORD.test(diagnostic.stage ?? "") ||
    Object.keys(diagnostic).some((key) => key !== "kind" && key !== "stage")
  ) {
    throw new Error(`${scenario.id} returned unsafe diagnostics`);
  }
}

function assertEvidence(evidence, scenario, payload) {
  assertEvidenceShape(evidence, scenario);
  assertRouteEvidence(evidence, scenario);
  assertPayload(evidence.payload);
  if (
    evidence.payload.bytes !== payload.bytes ||
    evidence.payload.sha256 !== payload.sha256
  ) {
    throw new Error(`${scenario.id} did not preserve payload integrity`);
  }
  assertCleanup(evidence, scenario);
  assertDiagnostic(evidence, scenario);
}

async function execute({ adapter, payload, scenario, time }) {
  const controller = new AbortController();
  const started = time.now();
  const deadline = started + time.timeoutMs;
  let timer = null;
  const expired = new Promise((resolve) => {
    timer = time.schedule(() => resolve({ expired: true }), time.timeoutMs);
  });
  try {
    const result = await Promise.race([
      adapter.execute(scenario, {
        deadline,
        payload,
        signal: controller.signal,
      }),
      expired,
    ]);
    if (result?.expired || time.now() > deadline) {
      controller.abort();
      throw new Error(`${scenario.id} exceeded its time limit`);
    }
    assertEvidence(result, scenario, payload);
    return { id: scenario.id, terminal: result.terminal };
  } finally {
    time.cancel(timer);
  }
}

function counts(cases) {
  return {
    cases: cases.length,
    outcomes: new Set(cases.map((scenario) => scenario.outcome)).size,
    pairs: new Set(cases.map((scenario) => scenario.pair)).size,
  };
}

export async function runConnectionsMatrix({
  adapter,
  context,
  pair,
  payload,
}) {
  if (!adapter || typeof adapter.execute !== "function") {
    throw new Error("Connections matrix needs an executable route adapter");
  }
  assertPayload(payload);
  const time = timing(context);
  const selected = matrixCases(pair);
  const cases = await selected.reduce(async (pending, scenario) => {
    const completed = await pending;
    completed.push(await execute({ adapter, payload, scenario, time }));
    return completed;
  }, Promise.resolve([]));
  return { cases, counts: counts(selected), schema: SCHEMA };
}
