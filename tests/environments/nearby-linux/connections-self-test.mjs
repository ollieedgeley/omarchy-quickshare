import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

const BINARY = "/usr/local/bin/file_share";
const FIXTURE_CONTENT = "omarchy-quickshare-connections-wifi-lan\n";
const MEDIUM = "wifi_lan";
const POLL_INTERVAL_MS = 100;
const RECEIVER_START_DELAY_MS = 500;
const SELF_TEST_TIMEOUT_MS = 20_000;
const WIFI_LAN_LOG = "WIFI_LAN";

function delay(milliseconds) {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}

function digest(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function command(role, payloadName) {
  const common = [BINARY, `--mediums=${MEDIUM}`, `--upgrade_mediums=${MEDIUM}`];
  if (role === "receiver") {
    return [...common, "--advertise", "--save_dir=/cases/received"];
  }
  return [...common, "--discover", `--send=/cases/outbound/${payloadName}`];
}

function prepareFixture(senderDirectory, receiverDirectory, name) {
  const outbound = join(senderDirectory, "outbound", name);
  const received = join(receiverDirectory, "received", name);
  mkdirSync(join(senderDirectory, "outbound"), { recursive: true });
  mkdirSync(join(receiverDirectory, "received"), { recursive: true });
  rmSync(outbound, { force: true });
  rmSync(received, { force: true });
  writeFileSync(outbound, FIXTURE_CONTENT);
  return { outbound, received };
}

function cleanFixture(fixture) {
  rmSync(fixture.outbound, { force: true });
  rmSync(fixture.received, { force: true });
  if (existsSync(fixture.outbound) || existsSync(fixture.received)) {
    throw new Error("Connections fixture cleanup did not complete");
  }
}

function evidenceFor(senderLog, receiverLog) {
  const connection = `${senderLog}\n${receiverLog}`;
  const evidence = {
    connection:
      connection.includes("Connection result") &&
      connection.includes("kSuccess"),
    discovery:
      senderLog.includes("OnEndpointFound") && senderLog.includes(WIFI_LAN_LOG),
    encryptedChannel: connection.includes("ENCRYPTED_WIFI_LAN"),
    receivedFile: receiverLog.includes("Received file"),
    sentPayload:
      senderLog.includes("SendPayload") &&
      senderLog.includes("status=kSuccess"),
  };
  if (Object.values(evidence).some((value) => !value)) {
    throw new Error("Connections Wi-Fi LAN log evidence is incomplete");
  }
  return evidence;
}

function transferReady(fixture, sender, receiver) {
  const matches =
    existsSync(fixture.received) &&
    digest(fixture.outbound) === digest(fixture.received);
  if (!matches) {
    return false;
  }
  try {
    evidenceFor(sender.logs(), receiver.logs());
    return true;
  } catch {
    return false;
  }
}

async function waitForTransfer(input) {
  if (transferReady(input.fixture, input.sender, input.receiver)) {
    return;
  }
  if (input.context.now() >= input.deadline) {
    throw new Error("Connections Wi-Fi LAN transfer exceeded its time limit");
  }
  await input.context.sleep(input.context.pollIntervalMs);
  await waitForTransfer(input);
}

async function stopProcesses(processes) {
  const results = await Promise.allSettled(
    processes.map((process) => process.stop()),
  );
  if (results.some(({ status }) => status === "rejected")) {
    throw new Error("Connections transfer processes did not stop cleanly");
  }
}

function assertContext(context) {
  if (
    !context ||
    typeof context.now !== "function" ||
    typeof context.pollIntervalMs !== "number" ||
    typeof context.sleep !== "function" ||
    typeof context.timeoutMs !== "number"
  ) {
    throw new Error("Connections self-test requires a complete timing context");
  }
}

function assertRunner(runner) {
  if (!runner || typeof runner.start !== "function") {
    throw new Error("Connections self-test requires a process runner");
  }
}

function flow(direction, receiver, sender) {
  return {
    direction,
    receiver,
    sender,
  };
}

async function transfer(runner, context, current) {
  const fixture = prepareFixture(
    current.sender.directory,
    current.receiver.directory,
    current.direction,
  );
  const receiver = runner.start({
    args: command("receiver", current.direction),
    peer: current.receiver.peer,
  });
  let sender = null;
  try {
    await context.sleep(context.receiverStartDelayMs);
    sender = runner.start({
      args: command("sender", current.direction),
      peer: current.sender.peer,
    });
    await waitForTransfer({
      context,
      deadline: context.now() + context.timeoutMs,
      fixture,
      receiver,
      sender,
    });
    const senderLog = sender.logs();
    const receiverLog = receiver.logs();
    return {
      bytes: readFileSync(fixture.outbound).byteLength,
      direction: current.direction,
      evidence: evidenceFor(senderLog, receiverLog),
      medium: MEDIUM,
      sha256: digest(fixture.outbound),
    };
  } finally {
    const processes = [receiver];
    if (sender) {
      processes.push(sender);
    }
    await stopProcesses(processes);
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

/**
 * The runner starts a peer command and returns `{ logs, stop }` functions.
 * The caller maps peer IDs to Compose `exec` commands after health checks pass.
 */
export async function runConnectionsSelfTest({ cases, context, runner }) {
  assertRunner(runner);
  const timing = normalizedContext(context);
  assertContext(timing);
  const first = flow(
    "a-to-b",
    { directory: cases.peerB, peer: "peer-b" },
    { directory: cases.peerA, peer: "peer-a" },
  );
  const second = flow(
    "b-to-a",
    { directory: cases.peerA, peer: "peer-a" },
    { directory: cases.peerB, peer: "peer-b" },
  );
  const transfers = [
    await transfer(runner, timing, first),
    await transfer(runner, timing, second),
  ];
  return { schema: 1, transfers };
}
