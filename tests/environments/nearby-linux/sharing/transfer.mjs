import { createHash } from "node:crypto";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { failureEvents } from "../compose-runner.mjs";

const EVENT_PATTERN = /^QS_EVENT (?<fields>.+)$/mu;
const RECEIVER_WAIT_MS = 2_000;
const TRANSFER_TIMEOUT_MS = 20_000;
const TRANSFER_COMPLETE_PERCENT = 100;
const PIN_SALT = "nearby-linux-sharing-self-test";
const WIFI_LAN_CHANNEL = "ENCRYPTED_WIFI_LAN";

function digest(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function delay(milliseconds) {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}

function field(value) {
  const [key, ...rest] = value.split("=");
  return [key, rest.join("=")];
}

export function parseEvents(log) {
  return log.split("\n").flatMap((line) => {
    const match = line.match(EVENT_PATTERN);
    if (!match) {
      return [];
    }
    return [Object.fromEntries(match.groups.fields.split(" ").map(field))];
  });
}

function transferFacts(log) {
  const allEvents = parseEvents(log);
  const events = allEvents.filter(({ event }) => event === "transfer");
  const completed = events.some(({ status }) => status === "kComplete");
  const token = allEvents.find(
    (event) => event.token_fingerprint,
  )?.token_fingerprint;
  const progress = Math.max(
    ...events.map((event) => Number(event.progress ?? 0)),
  );
  const total = Math.max(
    ...events.map((event) => Number(event.total_bytes ?? 0)),
  );
  if (!completed || !token || !Number.isFinite(total)) {
    const facts = [
      `complete=${completed}`,
      `token=${Boolean(token)}`,
      `total=${Number.isFinite(total)}`,
    ].join(",");
    const message = [
      "Nearby Sharing completion evidence is incomplete ",
      `(${facts})`,
    ].join("");
    throw new Error(message);
  }
  return { progress, token, total };
}

function assertTransfer({ received, receiverLog, senderLog, sent }) {
  const sender = transferFacts(senderLog);
  const receiver = transferFacts(receiverLog);
  const channel = `${senderLog}\n${receiverLog}`.includes(WIFI_LAN_CHANNEL);
  if (!existsSync(received)) {
    throw new Error("Nearby Sharing received file is missing");
  }
  const sentBytes = readFileSync(sent).byteLength;
  const receivedBytes = readFileSync(received).byteLength;
  const peerEvidenceAgrees = sender.token === receiver.token;
  const totalsAgree =
    sender.total === receiver.total &&
    sender.total === sentBytes &&
    receiver.total === receivedBytes;
  const progressComplete =
    sender.progress === TRANSFER_COMPLETE_PERCENT &&
    receiver.progress === TRANSFER_COMPLETE_PERCENT;
  if (!peerEvidenceAgrees || !totalsAgree || !progressComplete) {
    throw new Error("Nearby Sharing peer evidence does not agree");
  }
  if (!channel || digest(sent) !== digest(received)) {
    throw new Error("Nearby Sharing payload or LAN evidence does not agree");
  }
  return {
    bytes: sentBytes,
    medium: "wifi_lan",
    pinMatch: true,
    sha256: digest(sent),
  };
}

function prepareCase(senderDirectory, receiverDirectory, name) {
  const payload = join(senderDirectory, "outbound", name);
  const received = join(receiverDirectory, "received", name);
  rmSync(payload, { force: true });
  rmSync(received, { force: true });
  writeFileSync(payload, `Nearby Linux transfer ${name}\n`);
  return { payload, received };
}

function variables() {
  return {
    QUICKSHARE_PIN_SALT: PIN_SALT,
    XDG_CONFIG_HOME: "/run/quickshare/config",
    XDG_DOWNLOAD_DIR: "/cases/received",
    XDG_RUNTIME_DIR: "/run/quickshare",
    XDG_STATE_HOME: "/run/quickshare/state",
  };
}

function receiveCommand(flow) {
  return {
    args: [
      "/usr/local/bin/nearby_sharing_cli",
      "receive",
      "--action",
      "accept",
      "--name",
      flow.receiver.name,
      "--timeout",
      "20",
    ],
    peer: flow.receiver.peer,
    variables: variables(),
  };
}

function sendCommand(flow) {
  return {
    args: [
      "/usr/local/bin/nearby_sharing_cli",
      "send",
      `/cases/outbound/${flow.payloadName}`,
      "--name",
      flow.sender.name,
      "--timeout",
      "20",
    ],
    peer: flow.sender.peer,
    variables: variables(),
  };
}

function transferFailure(error, sender, receiver) {
  let senderEvents = 0;
  if (sender) {
    senderEvents = failureEvents(sender.logs()).length;
  }
  const receiverEvents = failureEvents(receiver.logs()).length;
  return new Error(
    "Nearby Sharing transfer failed " +
      `(sender events ${senderEvents}, receiver events ${receiverEvents})`,
    { cause: error },
  );
}

export async function stopPeers(peers) {
  await Promise.all(peers.filter(Boolean).map((peer) => peer.stop()));
}

async function transfer(options, flow) {
  const senderCase = options.cases[flow.sender.case];
  const receiverCase = options.cases[flow.receiver.case];
  const files = prepareCase(senderCase, receiverCase, flow.payloadName);
  const receiver = options.runner.start(receiveCommand(flow));
  let sender = null;
  try {
    await delay(RECEIVER_WAIT_MS);
    sender = options.runner.start(sendCommand(flow));
    await Promise.all([
      sender.wait({ timeoutMs: TRANSFER_TIMEOUT_MS }),
      receiver.wait({ timeoutMs: TRANSFER_TIMEOUT_MS }),
    ]);
    return assertTransfer({
      received: files.received,
      receiverLog: receiver.logs(),
      senderLog: sender.logs(),
      sent: files.payload,
    });
  } catch (error) {
    throw transferFailure(error, sender, receiver);
  } finally {
    await stopPeers([receiver, sender]);
    rmSync(files.payload, { force: true });
    rmSync(files.received, { force: true });
  }
}

export async function runSharingSelfTest(options) {
  const first = await transfer(options, {
    payloadName: "a-to-b.txt",
    receiver: { case: "peerB", name: "Peer-B", peer: "peer-b" },
    sender: { case: "peerA", name: "Peer-A", peer: "peer-a" },
  });
  const second = await transfer(options, {
    payloadName: "b-to-a.txt",
    receiver: { case: "peerA", name: "Peer-A", peer: "peer-a" },
    sender: { case: "peerB", name: "Peer-B", peer: "peer-b" },
  });
  return { first, second };
}
