import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { readFileSync } from "node:fs";
import { setTimeout as delay } from "node:timers/promises";

const COMMAND_TIMEOUT_MS = 5_000;
const STOP_GRACE_MS = 1_000;
const RETRY_MS = 10;
const START_ATTEMPTS = 100;
const PROC_START_TIME_INDEX = 19;

function daemonIdentity(child) {
  assert.equal(
    child.exitCode,
    null,
    "daemon exited during client interruption",
  );
  assert.equal(
    child.signalCode,
    null,
    "daemon was signalled during client interruption",
  );
  const stat = readFileSync(`/proc/${child.pid}/stat`, "utf8");
  const startTime = stat
    .slice(stat.lastIndexOf(")") + 2)
    .split(" ")
    .at(PROC_START_TIME_INDEX);
  return `${child.pid}:${startTime}`;
}

function nativeCommand(context, args, expectedStatus = 0) {
  const result = spawnSync(context.binary, args, {
    encoding: "utf8",
    env: { ...context.environment, OMARCHY_QUICKSHARE_ALLOW_SIMULATION: "1" },
    killSignal: "SIGKILL",
    timeout: COMMAND_TIMEOUT_MS,
  });
  assert.ifError(result.error);
  assert.equal(result.status, expectedStatus, result.stderr);
  return result.stdout;
}

function terminalBoundary(context, active) {
  const query = () =>
    JSON.parse(
      nativeCommand(context, ["operation", active.request_id, "--json"]),
    ).response.operation;
  nativeCommand(context, [
    "simulate",
    "progress",
    active.id_string,
    String(active.total_bytes),
  ]);
  const full = query();
  assert.equal(full.phase, "transferring");
  assert.equal(full.transferred_bytes, full.total_bytes);
  let first = ["simulate", "complete", active.id_string];
  let late = ["share", "cancel", active.id_string];
  let phase = "completed";
  if (context.terminalOrder === "cancel-first") {
    [first, late] = [late, first];
    phase = "cancelled";
  }
  nativeCommand(context, first);
  const winner = query();
  assert.equal(winner.phase, phase);
  assert.equal(winner.id_string, active.id_string);
  assert.equal(winner.request_id, active.request_id);
  if (["cancel-first", "complete-first"].includes(context.terminalOrder)) {
    nativeCommand(context, late, 1);
    assert.deepEqual(
      query(),
      winner,
      "late action rewrote the terminal outcome",
    );
  }
  const snapshot = JSON.parse(
    nativeCommand(context, ["status", "--json"]),
  ).response;
  assert.deepEqual(
    snapshot.operations,
    [winner],
    "expected exactly one retained terminal record",
  );
  assert.equal(snapshot.snapshot.active_share.phase, phase);
  return winner;
}

function clientReconnect(context, daemon, identity) {
  const logPath = context.prepared.env.SUBMISSION_ENVELOPES;
  const envelopes = () =>
    readFileSync(logPath, "utf8").trim().split("\n").map(JSON.parse);
  const submissions = envelopes().filter(({ request }) =>
    request.type.startsWith("submit_"),
  );
  assert.equal(submissions.length, 1);
  const requestId = submissions[0].request_id;
  const query = () =>
    JSON.parse(nativeCommand(context, ["operation", requestId, "--json"]))
      .response.operation;
  const active = query();
  assert.equal(active.phase, "transferring");
  assert.equal(active.request_id, requestId);
  assert.equal(daemonIdentity(daemon.child), identity);
  const terminal = terminalBoundary(context, active);
  for (const clientOutcome of ["visible", "dismissed"]) {
    context.prepared.env.CAPTURE_JOURNEY = JSON.stringify({
      clientOutcome,
      operationId: active.id_string,
      requestId,
      terminalPhase: terminal.phase,
    });
    context.runPanel();
    assert.equal(daemonIdentity(daemon.child), identity);
    assert.deepEqual(query(), { ...terminal, dismissed: true });
  }
  const requests = envelopes().map(({ request }) => request.type);
  assert.equal(requests.filter((type) => type.startsWith("submit_")).length, 1);
  assert.equal(requests.filter((type) => type === "dismiss").length, 1);
  process.stdout.write(
    `CLIENT_RECONNECTED daemon=${identity} operation=${active.id_string}` +
      ` request_id=${requestId} phase=${terminal.phase}\n`,
  );
}

export async function waitForDaemon(binary, environment, attempts) {
  const result = spawnSync(binary, ["health"], {
    env: environment,
    killSignal: "SIGKILL",
    timeout: COMMAND_TIMEOUT_MS,
  });
  assert.ifError(result.error);
  if (result.status === 0) {
    return;
  }
  assert.ok(attempts > 0, "isolated simulated daemon did not become ready");
  await delay(RETRY_MS);
  await waitForDaemon(binary, environment, attempts - 1);
}

export async function stopFixtureProcess(process) {
  if (
    !process ||
    process.child.exitCode !== null ||
    process.child.signalCode !== null
  ) {
    return;
  }
  process.child.kill("SIGTERM");
  const force = setTimeout(() => process.child.kill("SIGKILL"), STOP_GRACE_MS);
  let deadline = null;
  try {
    await Promise.race([
      process.exited,
      new Promise((resolve, reject) => {
        deadline = setTimeout(
          () => reject(new Error("Fixture child did not exit")),
          COMMAND_TIMEOUT_MS,
        );
      }),
    ]);
  } finally {
    clearTimeout(force);
    clearTimeout(deadline);
  }
}

export function assertReconciledSubmission(prepared) {
  const envelopes = readFileSync(prepared.env.SUBMISSION_ENVELOPES, "utf8")
    .trim()
    .split("\n")
    .map((line) => JSON.parse(line));
  const submissions = envelopes.filter(({ request }) =>
    request.type.startsWith("submit_"),
  );
  assert.equal(submissions.length, 1, "lost accepted reply must never resend");
  const queries = envelopes.filter(
    ({ request }) => request.type === "operation",
  );
  assert.ok(queries.length > 0, "lost reply must query durable operation");
  for (const { request } of queries) {
    assert.equal(request.request_id, submissions[0].request_id);
  }
  const queryIndex = envelopes.findIndex(
    ({ request }) => request.type === "operation",
  );
  const submitIndex = envelopes.indexOf(submissions[0]);
  assert.ok(
    envelopes
      .slice(submitIndex + 1, queryIndex)
      .some(({ request }) => request.type === "snapshot"),
    "reconciliation must refresh snapshot before querying operation",
  );
  return submissions[0].request_id;
}

async function restartForRecovery(context, daemon) {
  const { binary, daemonBinary, environment, prepared } = context;
  const requestId = assertReconciledSubmission(prepared);
  const status = spawnSync(binary, ["status", "--json"], {
    encoding: "utf8",
    env: environment,
    killSignal: "SIGKILL",
    timeout: COMMAND_TIMEOUT_MS,
  });
  assert.equal(status.status, 0, status.stderr);
  const latest = JSON.parse(status.stdout).response.operations.at(-1);
  assert.equal(latest.request_id, requestId);
  assert.equal(latest.phase, "awaiting_peer_consent");
  await stopFixtureProcess(daemon);
  const child = spawn(daemonBinary, ["--simulate"], {
    env: environment,
    stdio: "ignore",
  });
  return { child, exited: once(child, "exit"), operationId: latest.id_string };
}

function reloadOutcome(context, operationId) {
  const { prepared, runPanel } = context;
  for (const reload of ["visible", "dismissed"]) {
    prepared.env.CAPTURE_JOURNEY = JSON.stringify({ reload, operationId });
    runPanel();
  }
  assertReconciledSubmission(prepared);
}

function assertJourneyOutcome(prepared, journey, context) {
  const { binary, environment } = context;
  const status = spawnSync(binary, ["status", "--json"], {
    encoding: "utf8",
    env: environment,
    killSignal: "SIGKILL",
    timeout: COMMAND_TIMEOUT_MS,
  });
  assert.equal(status.status, 0, status.stderr);
  const share = JSON.parse(status.stdout).response.snapshot.active_share;
  if (
    journey.closedPaste ||
    journey.failReplacement ||
    (journey.invalidate && !journey.recover)
  ) {
    assert.equal(share, null);
  } else {
    assert.deepEqual(share.attachment, prepared.attachment);
    assert.equal(share.peer.id, prepared.peer);
  }
  let expectedReads = "";
  if (!journey.captureFirst && journey.automatic) {
    expectedReads = "read\n";
  }
  if (
    journey.replacement ||
    journey.peerChange ||
    journey.timeoutReplacement ||
    journey.invalidRead
  ) {
    expectedReads = "read\nread\n";
  }
  if (journey.failReplacement) {
    expectedReads = "read\nread\nread\n";
  }
  if (journey.invalidate) {
    expectedReads = "read\n";
  }
  assert.equal(readFileSync(prepared.env.CLIPBOARD_LOG, "utf8"), expectedReads);
  if (journey.failSubmission) {
    assert.equal(
      readFileSync(prepared.env.SUBMISSION_ATTEMPTS, "utf8"),
      "submit\nsubmit\n",
    );
  }
}

export async function observeJourney(context, journey, daemon) {
  let identity = null;
  if (journey.clientReconnect) {
    identity = daemonIdentity(daemon.child);
  }
  context.runPanel();
  assertJourneyOutcome(context.prepared, journey, context);
  if (journey.clientReconnect) {
    clientReconnect(
      { ...context, terminalOrder: journey.terminalOrder },
      daemon,
      identity,
    );
  }
  if (journey.submissionMode !== "drop") {
    return daemon;
  }
  const restarted = await restartForRecovery(context, daemon);
  try {
    await waitForDaemon(context.binary, context.environment, START_ATTEMPTS);
    reloadOutcome(context, restarted.operationId);
    return restarted;
  } catch (error) {
    await stopFixtureProcess(restarted);
    throw error;
  }
}
