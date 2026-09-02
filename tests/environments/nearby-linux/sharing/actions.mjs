import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { failureEvents } from "../compose-runner.mjs";
import { parseEvents } from "./transfer.mjs";

const BINARY = "/usr/local/bin/nearby_sharing_cli";
const POLL_INTERVAL_MS = 100;
const RECEIVER_START_DELAY_MS = 2_000;
const SELF_TEST_TIMEOUT_MS = 10_000;
const TERMINAL_STATUSES = new Set(["kCancelled", "kFailed", "kReject"]);

function delay(milliseconds) {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}

function actionCommand(flow) {
  const common = ["--name", flow.sender.name, "--timeout", "15"];
  const send = ["send", `/cases/outbound/${flow.payloadName}`, ...common];
  if (flow.action === "cancel") {
    send.push("--action", "cancel");
  }
  return [BINARY, ...send];
}

function receiveCommand(flow) {
  return [
    BINARY,
    "receive",
    "--action",
    flow.receiver.action,
    "--name",
    flow.receiver.name,
    "--timeout",
    "15",
  ];
}

function prepareFixture(sender, receiver, payloadName) {
  const outbound = join(sender.directory, "outbound", payloadName);
  const received = join(receiver.directory, "received", payloadName);
  mkdirSync(join(sender.directory, "outbound"), { recursive: true });
  mkdirSync(join(receiver.directory, "received"), { recursive: true });
  rmSync(outbound, { force: true });
  rmSync(received, { force: true });
  writeFileSync(outbound, `Nearby Sharing action ${payloadName}\n`);
  return { outbound, received };
}

function cleanFixture(fixture) {
  rmSync(fixture.outbound, { force: true });
  rmSync(fixture.received, { force: true });
  if (existsSync(fixture.outbound) || existsSync(fixture.received)) {
    throw new Error("Nearby Sharing action fixture cleanup did not complete");
  }
}

function expectedActionEvent(events, action, status) {
  return events.some(
    (event) =>
      event.event === "action-result" &&
      event.action === action &&
      event.status === status,
  );
}

function requestedAction(events, action) {
  return events.some(
    (event) => event.event === "action-requested" && event.action === action,
  );
}

function terminal(events, expected) {
  let allowed = [expected];
  if (Array.isArray(expected)) {
    allowed = expected;
  }
  return events.find(
    (event) =>
      event.event === "transfer" &&
      allowed.includes(event.status) &&
      TERMINAL_STATUSES.has(event.status),
  );
}

function assertTokens(senderEvents, receiverEvents) {
  const senderToken = senderEvents.find((event) => event.token)?.token;
  const receiverToken = receiverEvents.find((event) => event.token)?.token;
  if (!senderToken || !receiverToken || senderToken !== receiverToken) {
    throw new Error(
      "Nearby Sharing peers reported different confirmation tokens",
    );
  }
  return true;
}

function evidence(input) {
  const senderEvents = parseEvents(input.senderLog);
  const receiverEvents = parseEvents(input.receiverLog);
  const senderTerminal = terminal(senderEvents, input.flow.senderTerminal);
  let receiverTerminal = null;
  if (input.flow.receiverTerminal) {
    receiverTerminal = terminal(receiverEvents, input.flow.receiverTerminal);
  }
  if (!senderTerminal || (input.flow.receiverTerminal && !receiverTerminal)) {
    throw new Error("Nearby Sharing action lacks required terminal markers");
  }
  if (
    !requestedAction(receiverEvents, input.flow.receiver.action) ||
    !expectedActionEvent(
      receiverEvents,
      input.flow.receiver.action,
      input.flow.receiver.status,
    )
  ) {
    throw new Error(
      "Nearby Sharing receiver did not report its requested action",
    );
  }
  if (
    input.flow.action === "cancel" &&
    (!requestedAction(senderEvents, "cancel") ||
      !expectedActionEvent(senderEvents, "cancel", "kOk"))
  ) {
    throw new Error(
      "Nearby Sharing sender did not report outbound cancellation",
    );
  }
  if (existsSync(input.fixture.received)) {
    throw new Error(
      "Nearby Sharing action unexpectedly completed a received file",
    );
  }
  return {
    action: input.flow.action,
    completedFile: false,
    direction: input.flow.direction,
    pinMatch: assertTokens(senderEvents, receiverEvents),
    receiverTerminal: receiverTerminal?.status ?? null,
    senderTerminal: senderTerminal.status,
  };
}

function assertContext(context) {
  if (
    !context ||
    typeof context.now !== "function" ||
    typeof context.sleep !== "function" ||
    typeof context.pollIntervalMs !== "number" ||
    typeof context.timeoutMs !== "number"
  ) {
    throw new Error(
      "Nearby Sharing action self-test requires a timing context",
    );
  }
}

function assertRunner(runner) {
  if (!runner || typeof runner.start !== "function") {
    throw new Error(
      "Nearby Sharing action self-test requires a process runner",
    );
  }
}

function ready(input) {
  try {
    evidence({
      fixture: input.fixture,
      flow: input.flow,
      receiverLog: input.receiver.logs(),
      senderLog: input.sender.logs(),
    });
    return true;
  } catch {
    return false;
  }
}

function typedFailureEvidence(log) {
  const events = failureEvents(log);
  if (!events.length) {
    return "none";
  }
  return events.map(({ event, status }) => `${event}:${status}`).join(",");
}

function timeoutFailure(input) {
  const sender = typedFailureEvidence(input.sender.logs());
  const receiver = typedFailureEvidence(input.receiver.logs());
  return new Error(
    `Nearby Sharing ${input.flow.action} ${input.flow.direction} ` +
      `exceeded its time limit (sender events ${sender}, receiver events ` +
      `${receiver})`,
  );
}

async function waitForOutcome(input) {
  if (!ready(input)) {
    if (input.context.now() >= input.deadline) {
      throw timeoutFailure(input);
    }
    await input.context.sleep(input.context.pollIntervalMs);
    return waitForOutcome(input);
  }
  return null;
}

async function stop(processes) {
  const outcomes = await Promise.allSettled(
    processes.map((process) => process.stop()),
  );
  if (outcomes.some(({ status }) => status === "rejected")) {
    throw new Error("Nearby Sharing action processes did not stop cleanly");
  }
}

function peer(input) {
  return {
    action: input.action,
    directory: input.directory,
    name: input.name,
    peer: input.id,
    status: input.status,
  };
}

function actionStatus(action) {
  if (action === "hold") {
    return "held";
  }
  return "kOk";
}

function peerA(cases, action) {
  return peer({
    action,
    directory: cases.peerA,
    id: "peer-a",
    name: "Peer-A",
    status: actionStatus(action),
  });
}

function peerB(cases, action) {
  return peer({
    action,
    directory: cases.peerB,
    id: "peer-b",
    name: "Peer-B",
    status: actionStatus(action),
  });
}

function transferPeers(cases, input) {
  if (input.direction === "a-to-b") {
    return {
      receiver: peerB(cases, input.receiverAction),
      sender: peerA(cases, input.senderAction),
    };
  }
  return {
    receiver: peerA(cases, input.receiverAction),
    sender: peerB(cases, input.senderAction),
  };
}

function rejectFlow(cases, direction) {
  const roles = transferPeers(cases, {
    direction,
    receiverAction: "reject",
    senderAction: null,
  });
  return {
    action: "reject",
    direction,
    payloadName: `reject-${direction}.txt`,
    receiver: roles.receiver,
    receiverTerminal: "kReject",
    sender: roles.sender,
    senderTerminal: ["kFailed", "kReject"],
  };
}

function cancelFlow(cases, direction) {
  const roles = transferPeers(cases, {
    direction,
    receiverAction: "hold",
    senderAction: "cancel",
  });
  return {
    action: "cancel",
    direction,
    payloadName: `cancel-${direction}.txt`,
    receiver: roles.receiver,
    receiverTerminal: null,
    sender: roles.sender,
    senderTerminal: "kCancelled",
  };
}

function flows(cases) {
  return [
    rejectFlow(cases, "a-to-b"),
    rejectFlow(cases, "b-to-a"),
    cancelFlow(cases, "a-to-b"),
    cancelFlow(cases, "b-to-a"),
  ];
}

function variables() {
  return {
    XDG_CONFIG_HOME: "/run/quickshare/config",
    XDG_DOWNLOAD_DIR: "/cases/received",
    XDG_RUNTIME_DIR: "/run/quickshare",
    XDG_STATE_HOME: "/run/quickshare/state",
  };
}

async function runFlow(runner, context, flow) {
  const fixture = prepareFixture(flow.sender, flow.receiver, flow.payloadName);
  const receiver = runner.start({
    args: receiveCommand(flow),
    peer: flow.receiver.peer,
    variables: variables(),
  });
  let sender = null;
  try {
    await context.sleep(context.receiverStartDelayMs);
    sender = runner.start({
      args: actionCommand(flow),
      peer: flow.sender.peer,
      variables: variables(),
    });
    await waitForOutcome({
      context,
      deadline: context.now() + context.timeoutMs,
      fixture,
      flow,
      receiver,
      sender,
    });
    await Promise.all([
      receiver.wait({
        acceptedCodes: [1],
        timeoutMs: context.timeoutMs,
      }),
      sender.wait({ acceptedCodes: [1], timeoutMs: context.timeoutMs }),
    ]);
    return evidence({
      fixture,
      flow,
      receiverLog: receiver.logs(),
      senderLog: sender.logs(),
    });
  } finally {
    const processes = [receiver];
    if (sender) {
      processes.push(sender);
    }
    await stop(processes);
    cleanFixture(fixture);
  }
}

function normalizedContext(context = {}) {
  return {
    now: context.now ?? Date.now,
    pollIntervalMs: context.pollIntervalMs ?? POLL_INTERVAL_MS,
    receiverStartDelayMs:
      context.receiverStartDelayMs ?? RECEIVER_START_DELAY_MS,
    sleep: context.sleep ?? delay,
    timeoutMs: context.timeoutMs ?? SELF_TEST_TIMEOUT_MS,
  };
}

async function runFlows(input) {
  const [flow, ...next] = input.remaining;
  if (!flow) {
    return input.outcomes;
  }
  const outcome = await runFlow(input.runner, input.context, flow);
  return runFlows({
    context: input.context,
    outcomes: [...input.outcomes, outcome],
    remaining: next,
    runner: input.runner,
  });
}

/**
 * The runner starts a peer command and returns `{ logs, stop }` functions.
 * Rejected and cancelled terminal markers are expected CLI outcomes, not
 * harness errors.
 */
export async function runSharingActionsSelfTest({ cases, context, runner }) {
  assertRunner(runner);
  const timing = normalizedContext(context);
  assertContext(timing);
  const outcomes = await runFlows({
    context: timing,
    outcomes: [],
    remaining: flows(cases),
    runner,
  });
  return { expectedExit: "nonzero", outcomes, schema: 1 };
}
