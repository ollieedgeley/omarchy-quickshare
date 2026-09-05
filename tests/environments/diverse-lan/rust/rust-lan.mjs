import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  createComposeRunner,
  failureEvents,
} from "../../nearby-linux/compose-runner.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../../..");
const COMPOSE = join(DIRECTORY, "rust-compose.yaml");
const GOOGLE_MANIFEST = join(
  ROOT,
  "tests/environments/nearby-linux/environment.json",
);
const BINARY = "/usr/local/bin/omarchy-quickshare";
const CASE_DIRECTORY_MODE = 0o777;
const DISCOVERY_TIMEOUT_MS = 18_000;
const MULTI_FRAME_FILE_SIZE = 1_048_577;
const POLL_MS = 100;
const QUEUED_SHARE_PATTERN = /Share (?<id>\d+) queued/u;
const RECEIVED_SUBDIRECTORY = "omarchy-quickshare";
const RUST_LAN_IMAGE = "omarchy-quickshare/rust-lan-peer:development";
const SAFE_REASON_SEPARATOR = /\s+/u;
const SAFE_TERMINAL_REASONS = new Set(
  `cancelled collision connection_crypto connection_frame_too_large
connection_handshake connection_invalid_payload connection_io
connection_rejected connection_unexpected_frame connection_unknown
connection_wire disconnected interrupted invalid_advertisement invalid_frame
invalid_mdns_instance invalid_name invalid_offer invalid_payload mutation quota
rejected sharing_decode sharing_io size_mismatch timed_out unsupported`.split(
    SAFE_REASON_SEPARATOR,
  ),
);

function environment(directories) {
  const manifest = JSON.parse(readFileSync(GOOGLE_MANIFEST, "utf8"));
  return {
    ...process.env,
    GOOGLE_CASE_DIR: directories.google,
    NEARBY_LINUX_IMAGE: manifest.image,
    RUST_CASE_DIR: directories.rust,
    RUST_LAN_CONTEXT: ROOT,
  };
}

function command(arguments_, directories, capture = true) {
  return new Promise((resolve_, reject) => {
    const child = spawn(process.env.DOCKER ?? "docker", arguments_, {
      cwd: ROOT,
      env: environment(directories),
    });
    let output = "";
    child.stdout.on("data", (chunk) => {
      output += chunk;
    });
    child.stderr.on("data", (chunk) => {
      output += chunk;
    });
    child.once("error", reject);
    child.once("close", (code) => {
      if (code === 0 || !capture) {
        resolve_(output);
        return;
      }
      reject(new Error(output));
    });
  });
}

function compose(arguments_, directories, capture = true) {
  return command(
    ["compose", "--file", COMPOSE, ...arguments_],
    directories,
    capture,
  );
}

function caseDirectories() {
  const root = mkdtempSync(join(tmpdir(), "quickshare-rust-lan-"));
  const directories = {
    google: join(root, "google"),
    root,
    rust: join(root, "rust"),
  };
  for (const directory of [directories.google, directories.rust]) {
    mkdirSync(join(directory, "outbound"), { recursive: true });
    mkdirSync(join(directory, "received"), { recursive: true });
    chmodSync(directory, CASE_DIRECTORY_MODE);
    chmodSync(join(directory, "outbound"), CASE_DIRECTORY_MODE);
    chmodSync(join(directory, "received"), CASE_DIRECTORY_MODE);
  }
  return directories;
}

function rustCommand(arguments_, directories, capture = true) {
  return compose(
    ["exec", "--tty=false", "rust", BINARY, ...arguments_],
    directories,
    capture,
  );
}

async function status(directories) {
  const envelope = JSON.parse(
    await rustCommand(["status", "--json"], directories),
  );
  return envelope.response?.snapshot;
}

function recordLastSnapshot(directories, snapshot) {
  directories.lastSnapshot = snapshot;
}

function resetDiscoveryDeadline(directories) {
  directories.discoveryDeadline = Date.now() + DISCOVERY_TIMEOUT_MS;
}
function recordCheckpoint(directories, checkpoint) {
  directories.checkpoint = checkpoint;
}

function safeTerminalReason(snapshot) {
  const value = snapshot?.active_share?.terminal_reason;
  if (!value) {
    return null;
  }
  if (SAFE_TERMINAL_REASONS.has(value)) {
    return value;
  }
  return "other";
}

function snapshotEvidence(snapshot) {
  const active = snapshot?.active_share;
  return {
    attachmentType: active?.attachment?.type,
    direction: active?.direction,
    phase: active?.phase,
    terminalReason: safeTerminalReason(snapshot),
    totalBytes: active?.total_bytes,
    transferredBytes: active?.transferred_bytes,
  };
}

async function waitFor(directories, description, predicate) {
  recordCheckpoint(directories, description);
  const snapshot = await status(directories).catch((error) => {
    directories.statusError = error;
    return null;
  });
  if (snapshot && predicate(snapshot)) {
    recordLastSnapshot(directories, snapshot);
    return snapshot;
  }
  if (snapshot) {
    recordLastSnapshot(directories, snapshot);
  }
  if (Date.now() >= directories.discoveryDeadline) {
    const evidence = JSON.stringify(snapshotEvidence(directories.lastSnapshot));
    throw new Error(`Rust daemon did not ${description}; last=${evidence}`);
  }
  await new Promise((resolve_) => {
    setTimeout(resolve_, POLL_MS);
  });
  return waitFor(directories, description, predicate);
}

function peerId(snapshot) {
  const peer = snapshot.peers?.at(0);
  assert.ok(peer, "Rust daemon did not report the reference mDNS peer");
  return peer.id;
}

function transferHash(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function valueHash(value) {
  return createHash("sha256").update(value).digest("hex");
}

function contentValue(kind) {
  if (kind === "text") {
    return "reference text payload";
  }
  if (kind === "url") {
    return "https://example.test";
  }
  return Buffer.alloc(MULTI_FRAME_FILE_SIZE, "G");
}
function outboundFixtureDirectory(directories, direction) {
  if (direction === "rust-to-google") {
    return directories.rust;
  }
  return directories.google;
}

function contentFixture(directories, direction, kind) {
  const file = `${direction}-${kind}-${randomUUID()}.txt`;
  const source = join(
    outboundFixtureDirectory(directories, direction),
    "outbound",
    file,
  );
  const value = contentValue(kind);
  writeFileSync(source, value);
  return { file, source, value };
}
function retryBytes(kind, retry, value) {
  if (retry && kind === "url") {
    return value;
  }
  return null;
}

function googleVariables({ retry = false, retryBody, textOutput } = {}) {
  const variables = {
    QUICKSHARE_PIN_SALT: "rust-lan",
    XDG_CONFIG_HOME: "/run/quickshare/config",
    XDG_DOWNLOAD_DIR: "/cases/received",
    XDG_RUNTIME_DIR: "/run/quickshare",
    XDG_STATE_HOME: "/run/quickshare/state",
  };
  if (retry) {
    variables.QUICKSHARE_INJECT_RETRY12 = "1";
  }
  if (retryBody) {
    variables.QUICKSHARE_INJECT_RETRY12_BYTES = retryBody;
  }
  if (textOutput) {
    variables.QUICKSHARE_TEXT_OUTPUT = textOutput;
  }
  return variables;
}

function frameDispatch12(log) {
  return (
    log.includes('stage="frame_dispatch"') && log.includes("frame_type_code=12")
  );
}

function assertRetry(directories, log, position) {
  const expected = new RegExp(
    `QS_EVENT event=control-frame frame_type=bandwidth-upgrade-retry ` +
      `position=${position}`,
    "u",
  );
  assert.match(log, expected);
  assert.ok(frameDispatch12(directories.daemonLogs()));
}
function keepaliveAck(log) {
  return (
    log.includes('stage="control"') &&
    log.includes('operation="send"') &&
    log.includes('outcome="locally_written"') &&
    log.includes('frame_type="keepalive_ack"')
  );
}

function assertKeepaliveSequence(directories, log, position) {
  const expected = new RegExp(
    `QS_EVENT event=control-frame frame_type=keepalive ` +
      `position=${position}`,
    "u",
  );
  assert.match(log, expected);
  assert.ok(directories.daemonLogs().includes("keepalive received"));
}
function boundedErrorReason(error) {
  if (error.code === "ENOENT") {
    return "missing-persisted-content";
  }
  if (error.code === "ERR_ASSERTION") {
    return "assertion-failed";
  }
  if (error.message.includes("exceeded its time limit")) {
    return "reference-timeout";
  }
  if (error.message.startsWith("Rust daemon did not")) {
    return "local-timeout";
  }
  return "unexpected-error";
}

async function scenarioFailure(error, directories, peer) {
  const latest = await status(directories).catch(() => null);
  if (latest) {
    recordLastSnapshot(directories, latest);
  }
  const daemon = directories.daemonLogs();
  const evidence = {
    checkpoint: directories.checkpoint,
    daemon: {
      explicitDisconnect: daemon.includes('disconnect_origin="explicit_frame"'),
      frameDispatch12: frameDispatch12(daemon),
      keepaliveAck: keepaliveAck(daemon),
      keepaliveReceived: daemon.includes("keepalive received"),
      sendDuringRejected:
        daemon.includes('stage="framing"') &&
        daemon.includes('outcome="rejected"'),
      streamEof: daemon.includes('disconnect_origin="stream_eof"'),
      unexpectedFrameType: daemon.includes('reason="unexpected_frame_type"'),
    },
    errorType: error.constructor.name,
    local: snapshotEvidence(directories.lastSnapshot),
    peerStopped: directories.peerStopped === true,
    reason: boundedErrorReason(error),
    reference: failureEvents(peer.logs()),
  };
  return new Error(`Rust LAN scenario failed: ${JSON.stringify(evidence)}`);
}

function startGoogleReceiver(
  directories,
  { action = "accept", retry = false, retryBody, textOutput } = {},
) {
  const runner = createComposeRunner({
    compose: COMPOSE,
    docker: process.env.DOCKER ?? "docker",
    environment: environment(directories),
  });
  return runner.start({
    args: [
      "/usr/local/bin/nearby_sharing_cli",
      "receive",
      "--action",
      action,
      "--name",
      "Google-B",
      "--timeout",
      "18",
    ],
    peer: "google",
    variables: googleVariables({ retry, retryBody, textOutput }),
  });
}

function startGoogleSender(
  directories,
  file,
  { kind = "file", retry = false, retryBody } = {},
) {
  const runner = createComposeRunner({
    compose: COMPOSE,
    docker: process.env.DOCKER ?? "docker",
    environment: environment(directories),
  });
  return runner.start({
    args: [
      "/usr/local/bin/nearby_sharing_cli",
      "send",
      `/cases/outbound/${file}`,
      "--content-kind",
      kind,
      "--name",
      "Google-A",
      "--timeout",
      "18",
    ],
    peer: "google",
    variables: googleVariables({ retry, retryBody }),
  });
}

function assertGoogleToRustContent({ directories, fixture, kind, snapshot }) {
  if (kind === "file") {
    const received = join(
      directories.rust,
      "received",
      RECEIVED_SUBDIRECTORY,
      fixture.file,
    );
    assert.equal(transferHash(fixture.source), transferHash(received));
    return;
  }
  assert.equal(snapshot.active_share.attachment.type, kind);
  assert.equal(
    valueHash(snapshot.active_share.attachment.value),
    transferHash(fixture.source),
  );
}
function assertNoInboundFile(directories, fixture) {
  const received = join(
    directories.rust,
    "received",
    RECEIVED_SUBDIRECTORY,
    fixture.file,
  );
  assert.equal(
    existsSync(received),
    false,
    "Rust daemon persisted file bytes after a terminal non-completion",
  );
}

function inboundAction(outcome) {
  if (outcome === "completed") {
    return "accept";
  }
  if (outcome === "rejected") {
    return "reject";
  }
  return "cancel";
}
function acceptedExitCodes(outcome) {
  if (outcome === "completed") {
    return [0];
  }
  return [1];
}

async function finishGoogleToRust(options) {
  const { directories, outcome, sender, shareId } = options;
  if (outcome === "failed") {
    await sender.stop();
    directories.peerStopped = true;
  } else {
    await rustCommand(["share", inboundAction(outcome), shareId], directories);
    await sender.wait({
      acceptedCodes: acceptedExitCodes(outcome),
      timeoutMs: DISCOVERY_TIMEOUT_MS,
    });
  }
  resetDiscoveryDeadline(directories);
  return waitFor(
    directories,
    `report the inbound share as ${outcome}`,
    (value) =>
      String(value.active_share?.id) === shareId &&
      value.active_share?.phase === outcome,
  );
}

async function assertGoogleToRust(directories, options) {
  const { kind, outcome, retry } = options;
  const fixture = contentFixture(directories, "google-to-rust", kind);
  await rustCommand(["visibility", "open"], directories);
  const sender = startGoogleSender(directories, fixture.file, {
    kind,
    retry,
    retryBody: retryBytes(kind, retry, fixture.value),
  });
  try {
    const offered = await waitFor(
      directories,
      `report the inbound ${kind} offer`,
      (value) => value.active_share?.direction === "inbound",
    );
    const completed = await finishGoogleToRust({
      directories,
      outcome,
      sender,
      shareId: String(offered.active_share.id),
    });
    if (outcome === "completed") {
      assertGoogleToRustContent({
        directories,
        fixture,
        kind,
        snapshot: completed,
      });
    } else {
      assertNoInboundFile(directories, fixture);
    }
    if (retry) {
      recordCheckpoint(directories, "verify Retry12 interleaving");
      assertRetry(directories, sender.logs(), "before-payload-data");
      if (kind === "url") {
        recordCheckpoint(directories, "verify keepalive acknowledgment");
        assertKeepaliveSequence(
          directories,
          sender.logs(),
          "before-payload-data",
        );
        assert.ok(keepaliveAck(directories.daemonLogs()));
      }
    }
  } catch (error) {
    throw await scenarioFailure(error, directories, sender);
  } finally {
    await sender.stop();
  }
}

async function queueRustOutbound(directories, content, kind) {
  const snapshot = await waitFor(
    directories,
    "report the reference mDNS peer",
    (value) => value.peers?.length,
  );
  recordCheckpoint(directories, "queue outbound attachment");
  const queued = await rustCommand(["send", content], directories);
  const shareId = queued.match(QUEUED_SHARE_PATTERN)?.groups.id;
  assert.ok(shareId, "Rust daemon did not queue an outbound share");
  const queuedSnapshot = await waitFor(
    directories,
    "report the queued outbound attachment",
    (value) => String(value.active_share?.id) === shareId,
  );
  recordCheckpoint(directories, "verify queued attachment type");
  assert.equal(queuedSnapshot.active_share.attachment.type, kind);
  recordCheckpoint(directories, "select reference peer");
  await rustCommand(
    ["share", "select", shareId, peerId(snapshot)],
    directories,
  );
  return shareId;
}

function assertRustOutboundBytes({ directories, fixture, outcome }) {
  const received = join(directories.google, "received", fixture.file);
  if (outcome === "completed") {
    assert.equal(transferHash(fixture.source), transferHash(received));
    return;
  }
  assert.equal(
    existsSync(received),
    false,
    "Reference peer persisted content after rejecting the share",
  );
}
function assertReferenceContentKind({ fixture, kind, log, outcome }) {
  if (kind === "file" || outcome !== "completed") {
    return;
  }
  const bytes = readFileSync(fixture.source).byteLength;
  const event = new RegExp(
    `QS_EVENT event=attachment kind=${kind} bytes=${bytes}`,
    "u",
  );
  assert.match(log, event);
}

function rustOutboundInput(fixture, kind) {
  if (kind === "file") {
    return {
      content: `/cases/outbound/${fixture.file}`,
      textOutput: null,
    };
  }
  return {
    content: fixture.value,
    textOutput: `/cases/received/${fixture.file}`,
  };
}

function referenceOutcome(outcome) {
  if (outcome === "completed") {
    return { acceptedCodes: [0], action: "accept" };
  }
  if (outcome === "rejected") {
    return { acceptedCodes: [1], action: "reject" };
  }
  return { acceptedCodes: [1], action: "hold" };
}

async function finishRustToGoogle(options) {
  const { directories, outcome, receiver, shareId } = options;
  if (outcome === "cancelled" || outcome === "failed") {
    await waitFor(
      directories,
      "await reference consent",
      (value) => value.active_share?.phase === "awaiting_peer_consent",
    );
  }
  if (outcome === "failed") {
    await receiver.stop();
    directories.peerStopped = true;
  } else {
    if (outcome === "cancelled") {
      await rustCommand(["share", "cancel", shareId], directories);
    }
    recordCheckpoint(directories, "wait for reference terminal status");
    await receiver.wait({
      acceptedCodes: acceptedExitCodes(outcome),
      timeoutMs: DISCOVERY_TIMEOUT_MS,
    });
  }
  resetDiscoveryDeadline(directories);
  return waitFor(
    directories,
    `report the outbound share as ${outcome}`,
    (value) =>
      String(value.active_share?.id) === shareId &&
      value.active_share?.phase === outcome,
  );
}

async function assertRustToGoogle(directories, options) {
  const { kind, outcome, retry } = options;
  const fixture = contentFixture(directories, "rust-to-google", kind);
  const input = rustOutboundInput(fixture, kind);
  await rustCommand(["discover", "start"], directories);
  const reference = referenceOutcome(outcome);
  const receiver = startGoogleReceiver(directories, {
    action: reference.action,
    retry,
    retryBody: retryBytes(kind, retry, fixture.value),
    textOutput: input.textOutput,
  });
  try {
    const shareId = await queueRustOutbound(directories, input.content, kind);
    await finishRustToGoogle({
      directories,
      outcome,
      receiver,
      shareId,
    });
    recordCheckpoint(directories, "verify reference persisted bytes");
    assertRustOutboundBytes({ directories, fixture, outcome });
    recordCheckpoint(directories, "verify reference attachment kind");
    assertReferenceContentKind({
      fixture,
      kind,
      log: receiver.logs(),
      outcome,
    });
    if (retry) {
      recordCheckpoint(directories, "verify Retry12 interleaving");
      assertRetry(directories, receiver.logs(), "after-first-payload-data");
      if (kind === "url") {
        recordCheckpoint(directories, "verify keepalive processing");
        assertKeepaliveSequence(
          directories,
          receiver.logs(),
          "after-first-payload-data",
        );
      }
    }
  } catch (error) {
    throw await scenarioFailure(error, directories, receiver);
  } finally {
    await receiver.stop();
  }
}

function startDaemon(directories) {
  const child = spawn(
    process.env.DOCKER ?? "docker",
    [
      "compose",
      "--file",
      COMPOSE,
      "exec",
      "--tty=false",
      "rust",
      BINARY,
      "daemon",
      "--log-level",
      "trace",
    ],
    { cwd: ROOT, env: environment(directories) },
  );
  let logs = "";
  child.stdout.on("data", (chunk) => {
    logs += chunk;
  });
  child.stderr.on("data", (chunk) => {
    logs += chunk;
  });
  const completed = new Promise((resolve_, reject) => {
    child.once("error", reject);
    child.once("close", resolve_);
  });
  return {
    logs: () => logs,
    stop: async () => {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
      }
      await completed;
    },
  };
}
function removeCaseRoot(root) {
  try {
    rmSync(root, { force: true, recursive: true });
    return;
  } catch (error) {
    if (error.code !== "EACCES") {
      throw error;
    }
  }
  const result = spawnSync(process.env.DOCKER ?? "docker", [
    "run",
    "--rm",
    "--network",
    "none",
    "--user",
    "0",
    "--entrypoint",
    "/bin/chmod",
    "--volume",
    `${root}:/wipe`,
    RUST_LAN_IMAGE,
    "-R",
    "a+rwx",
    "/wipe",
  ]);
  if (result.status !== 0) {
    throw new Error(
      result.stderr.toString() ||
        "could not make rust LAN case files removable",
    );
  }
  rmSync(root, { force: true, recursive: true });
}
async function cleanupScenario(directories, daemon) {
  let cleanupError = null;
  try {
    if (daemon) {
      await daemon.stop();
    }
  } catch (error) {
    cleanupError = error;
  }
  try {
    await compose(["down", "--remove-orphans", "--volumes"], directories);
  } catch (error) {
    cleanupError ??= error;
  }
  try {
    removeCaseRoot(directories.root);
  } catch (error) {
    cleanupError ??= error;
  }
  return cleanupError;
}

export async function runRustLanScenario({
  direction,
  kind = "file",
  outcome = "completed",
  retry = false,
}) {
  assert.ok(direction === "rust-to-google" || direction === "google-to-rust");
  assert.ok(["file", "text", "url"].includes(kind));
  assert.ok(["completed", "rejected", "cancelled", "failed"].includes(outcome));
  assert.equal(typeof retry, "boolean");
  const directories = caseDirectories();
  let daemon = null;
  let failure = null;
  try {
    await compose(
      ["up", "--detach", "--no-build", "--wait", "--wait-timeout", "30"],
      directories,
    );
    daemon = startDaemon(directories);
    directories.daemonLogs = daemon.logs;
    resetDiscoveryDeadline(directories);
    await waitFor(directories, "start its control listener", () => true);
    if (direction === "rust-to-google") {
      await assertRustToGoogle(directories, { kind, outcome, retry });
    } else {
      await assertGoogleToRust(directories, { kind, outcome, retry });
    }
  } catch (error) {
    failure = error;
  } finally {
    const cleanupFailure = await cleanupScenario(directories, daemon);
    failure ??= cleanupFailure;
  }
  if (failure) {
    throw failure;
  }
}

export async function provisionRustLan() {
  const directories = caseDirectories();
  try {
    await compose(["build", "rust"], directories);
  } finally {
    rmSync(directories.root, { force: true, recursive: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== "provision") {
    throw new Error("usage: node rust-lan.mjs provision");
  }
  await provisionRustLan();
}
