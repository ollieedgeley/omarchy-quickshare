import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import * as failure from "../../../tools/gates/lib/failure-artifact.mjs";

import {
  contextFingerprint,
  contextPath,
  contextRelativePath,
  treeFingerprint,
} from "./context.mjs";
import {
  composeArguments,
  createComposeRunner,
  failureEvents,
} from "./compose-runner.mjs";
import { runConnectionsSelfTest } from "./connections-self-test.mjs";
import {
  buildEnvironment,
  environmentFingerprint,
  runTimedSelfTest,
  validateEnvironment,
} from "./environment.mjs";
import { runSharingActionsSelfTest } from "./sharing/actions.mjs";
import * as sharingTest from "./sharing/transfer.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "nearby-linux");
const BASE_IMAGE_PATTERN = /^ubuntu@sha256:[0-9a-f]{64}$/u;
const CONTEXT_ROOT = "/tmp/nearby-linux";
const FINGERPRINT =
  "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const MISSING_INPUT_PATTERN = /source inputs are incomplete/u;
const PIN_ERROR_PATTERN = /base image must use a SHA-256 digest/u;
const CONTEXT_CHILD_PATTERN = /context child/u;
const FAKE_TIMEOUT_MS = 10;
const TIMER_TICKS = 4;
const CONTROL_ACTION_INDEX = -3;
const PROCESS_IS_RUNNING = 0;
const PROCESS_IS_STOPPED = 1;
const PEER_PARSE_FAILURE = 23;
const MARKER_OFFSET = 1;
const MARKER_PATH_PATTERN = /^\/run\/quickshare\/commands\/.+\.pid$/u;
const UNKNOWN_FAILURE_ARTIFACT_KEY = /unknown failure artifact key/u;
const CONNECTION_CONTRACT_TEST =
  "Connections contracts both Wi-Fi LAN directions and cleans fixtures";
const SHARING_TRANSFER_TEST =
  "Sharing completes transfer evidence in both directions";
const SHARING_ACTION_TEST =
  "Sharing actions contract rejects and cancels in both directions";

function fakeComposeProcess() {
  const child = new EventEmitter();
  child.exitCode = null;
  child.signalCode = null;
  child.stderr = new EventEmitter();
  child.stdout = new EventEmitter();
  child.kill = () => {
    child.exitCode = 0;
    child.emit("close", 0, null);
  };
  return child;
}

function fakeComposeSpawn(calls) {
  let client = null;
  let peerIsLive = true;
  return (_docker, args) => {
    calls.push(args);
    const child = fakeComposeProcess();
    if (args.includes("setsid")) {
      client = child;
      return child;
    }
    queueMicrotask(() => {
      const action = args.at(CONTROL_ACTION_INDEX);
      const signal = args.at(-1);
      if (action === "signal" && signal === "KILL") {
        peerIsLive = false;
      }
      let code = PROCESS_IS_RUNNING;
      if (action === "probe" && !peerIsLive) {
        code = PROCESS_IS_STOPPED;
      }
      child.exitCode = code;
      child.emit("close", code, null);
      if (!peerIsLive && client) {
        client.kill();
      }
    });
    return child;
  };
}

function failedComposeSpawn(code) {
  return () => {
    const child = fakeComposeProcess();
    queueMicrotask(() => {
      child.exitCode = code;
      child.emit("close", code, null);
    });
    return child;
  };
}

function inputs() {
  return {
    assets: treeFingerprint(join(DIRECTORY, "assets")),
    compose: readFileSync(join(DIRECTORY, "compose.yaml"), "utf8"),
    connectionsPeer: treeFingerprint(
      join(ROOT, "tools", "oracle", "connections-peer"),
    ),
    contextSource: readFileSync(join(DIRECTORY, "context.mjs"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile"), "utf8"),
    fixtureGenerator: treeFingerprint(
      join(ROOT, "tools", "oracle", "sharing-fixtures"),
    ),
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    overlays: treeFingerprint(
      join(ROOT, "tests", "environments", "oracle", "overlays"),
    ),
    patch: readFileSync(join(DIRECTORY, "cli-actions.patch"), "utf8"),
    sources: readFileSync(join(ROOT, "upstream", "sources.toml"), "utf8"),
  };
}

test("Nearby Linux pins its image, source inputs, and build context", () => {
  const { manifest, ...files } = inputs();
  const input = { ...files, manifestSource: manifest };
  const parsed = validateEnvironment(input);
  assert.match(parsed.ubuntu.base, BASE_IMAGE_PATTERN);
  assert.ok(parsed.sources.includes("gloop"));
  assert.ok(parsed.sources.includes("sdbus-cpp"));
  assert.equal(environmentFingerprint(input), contextFingerprint(input));
  assert.ok(existsSync(join(DIRECTORY, "assets", "peer-entrypoint.sh")));
});

test("Nearby Linux waits for a managed local network", () => {
  const configuration = readFileSync(
    join(DIRECTORY, "assets", "NetworkManager.conf"),
    "utf8",
  );
  const compose = readFileSync(join(DIRECTORY, "compose.yaml"), "utf8");
  assert.ok(configuration.includes("unmanaged-devices=\n"));
  assert.ok(compose.includes("nmcli --get-values STATE general"));
});

test("Nearby Linux rejects a mutable image and missing build source", () => {
  const { manifest, ...files } = inputs();
  const mutable = JSON.stringify({
    ...JSON.parse(manifest),
    ubuntu: { ...JSON.parse(manifest).ubuntu, base: "ubuntu:noble" },
  });
  assert.throws(
    () =>
      validateEnvironment({
        ...files,
        manifestSource: mutable,
      }),
    PIN_ERROR_PATTERN,
  );
  const missing = JSON.stringify({
    ...JSON.parse(manifest),
    sources: ["gloop"],
  });
  assert.throws(
    () =>
      validateEnvironment({
        ...files,
        manifestSource: missing,
      }),
    MISSING_INPUT_PATTERN,
  );
});

test("Nearby Linux Compose receives only a sealed build context", () => {
  const { manifest, ...files } = inputs();
  const parsed = validateEnvironment({
    ...files,
    manifestSource: manifest,
  });
  const context = contextPath(CONTEXT_ROOT, FINGERPRINT);
  const environment = buildEnvironment(parsed, FINGERPRINT, context);
  assert.equal(environment.NEARBY_LINUX_BUILD_CONTEXT, context);
  assert.equal(contextRelativePath(context, join(context, "nearby")), "nearby");
  assert.throws(
    () => contextRelativePath(context, CONTEXT_ROOT),
    CONTEXT_CHILD_PATTERN,
  );
});

test("Nearby Linux fingerprint changes with every tracked input class", () => {
  const { manifest, ...files } = inputs();
  const input = { ...files, manifestSource: manifest };
  const original = environmentFingerprint(input);
  for (const name of [
    "assets",
    "compose",
    "contextSource",
    "dockerfile",
    "manifestSource",
    "overlays",
    "patch",
    "sources",
  ]) {
    assert.notEqual(
      environmentFingerprint({ ...input, [name]: `${input[name]}changed` }),
      original,
    );
  }
});

test("pinned Compose values override the invoking host environment", () => {
  const { manifest, ...files } = inputs();
  const parsed = validateEnvironment({
    ...files,
    manifestSource: manifest,
  });
  const hadPrevious = Object.hasOwn(process.env, "NEARBY_LINUX_IMAGE");
  const previous = process.env.NEARBY_LINUX_IMAGE;
  process.env.NEARBY_LINUX_IMAGE = "untrusted:latest";
  try {
    const environment = buildEnvironment(parsed, FINGERPRINT, CONTEXT_ROOT);
    assert.equal(environment.NEARBY_LINUX_IMAGE, parsed.image);
    assert.equal(environment.NEARBY_LINUX_DOCKERFILE, "Dockerfile");
  } finally {
    if (hadPrevious) {
      process.env.NEARBY_LINUX_IMAGE = previous;
    } else {
      delete process.env.NEARBY_LINUX_IMAGE;
    }
  }
});

test("Compose runner drops peer commands to the test user", () => {
  const argumentsList = composeArguments("compose.yaml", {
    args: ["/usr/local/bin/file_share", "--advertise"],
    peer: "peer-a",
    variables: { XDG_RUNTIME_DIR: "/run/quickshare" },
  });
  assert.ok(argumentsList.includes("--tty=false"));
  assert.ok(argumentsList.includes("quickshare"));
  assert.ok(argumentsList.includes("XDG_RUNTIME_DIR=/run/quickshare"));
});

test("Compose runner scopes peer cleanup to its started command", async () => {
  const calls = [];
  const runner = createComposeRunner({
    compose: "compose.yaml",
    docker: "docker",
    environment: process.env,
    spawn: fakeComposeSpawn(calls),
    stopMs: 1,
  });
  const peerProcess = runner.start({
    args: ["/usr/local/bin/file_share", "--advertise"],
    peer: "peer-a",
  });
  assert.ok(calls[0].includes("setsid"));
  assert.ok(
    calls[0].some(
      (argument) =>
        typeof argument === "string" && argument.includes("umask 022"),
    ),
  );
  await peerProcess.stop();
  const marker = calls[0][calls[0].indexOf("compose-runner") + MARKER_OFFSET];
  assert.match(marker, MARKER_PATH_PATTERN);
  assert.ok(calls.slice(1).every((args) => args.includes(marker)));
  assert.ok(calls.some((args) => args.includes("INT")));
  assert.ok(calls.some((args) => args.includes("KILL")));
  assert.ok(calls.some((args) => args.includes("probe")));
  assert.ok(calls.some((args) => args.includes("verify")));
});

test("Compose runner keeps only typed peer failure events", () => {
  const events = failureEvents(
    "noise\nQS_EVENT event=transfer token=1234 status=kFailed\n" +
      "connection auth_digits=ABCDE failed\nconfirmation token: 1234\n",
  );
  assert.deepEqual(events, [{ event: "transfer", status: "failed" }]);
});

test("Compose runner drops malformed peer failure events", () => {
  const events = failureEvents("QS_EVENT event=___ status=kFailed\n");
  assert.deepEqual(events, []);
});

test("Compose runner retains safe terminal outcome categories", () => {
  const events = failureEvents(
    "QS_EVENT event=transfer status=kReject\n" +
      "QS_EVENT event=transfer status=kCancelled\n" +
      "QS_EVENT event=action-result status=kOk\n" +
      "QS_EVENT event=transfer status=kAwaitingRemoteAcceptance\n",
  );
  assert.deepEqual(events, [
    { event: "transfer", status: "rejected" },
    { event: "transfer", status: "cancelled" },
    { event: "action-result", status: "succeeded" },
    { event: "transfer", status: "awaiting-remote-acceptance" },
  ]);
});

test("Compose runner reports an in-container peer failure code", async () => {
  const runner = createComposeRunner({
    compose: "compose.yaml",
    docker: "docker",
    environment: process.env,
    spawn: failedComposeSpawn(PEER_PARSE_FAILURE),
  });
  const peerProcess = runner.start({
    args: ["/usr/local/bin/file_share", "--invalid"],
    peer: "peer-a",
  });
  await assert.rejects(peerProcess.wait({ timeoutMs: 1 }), (error) => {
    assert.equal(
      error.message,
      `Nearby Linux peer command exited with ${PEER_PARSE_FAILURE}`,
    );
    return true;
  });
});

test("failure artifacts reject secret-bearing diagnostics", () => {
  const root = mkdtempSync(join(tmpdir(), "failure-artifact-"));
  try {
    const artifact = failure.writeFailureArtifact(root, {
      events: [{ event: "transfer", status: "failed" }],
      gate: "nearby-linux",
      outcome: { kind: "timeout" },
      stage: "peer-command",
    });
    assert.deepEqual(JSON.parse(readFileSync(artifact, "utf8")), {
      events: [{ event: "transfer", status: "failed" }],
      gate: "nearby-linux",
      outcome: { kind: "timeout" },
      schema: 1,
      stage: "peer-command",
    });
    assert.throws(
      () =>
        failure.recordFailureArtifact(root, {
          gate: "nearby-linux",
          outcome: { kind: "timeout", token: "1234" },
          stage: "peer-command",
        }),
      UNKNOWN_FAILURE_ARTIFACT_KEY,
    );
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("suite assertion failures retain typed evidence", async () => {
  const root = mkdtempSync(join(tmpdir(), "nearby-suite-failure-"));
  const expected = new Error("secret peer output");
  try {
    await assert.rejects(
      runTimedSelfTest("sharing-actions", () => Promise.reject(expected), {
        context: {},
        failureDirectory: root,
      }),
      expected,
    );
    const artifact = join(root, "nearby-linux-failure.json");
    assert.deepEqual(JSON.parse(readFileSync(artifact, "utf8")), {
      events: [{ event: "suite", status: "failed" }],
      gate: "nearby-linux",
      outcome: { kind: "failed" },
      schema: 1,
      stage: "sharing-actions",
    });
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("Sharing event parser preserves typed transfer evidence", () => {
  const events = sharingTest.parseEvents(
    "QS_EVENT event=transfer status=kComplete target_id=1 " +
      "transferred_bytes=7 total_bytes=7 token=1234\n",
  );
  assert.equal(events.length, 1);
  assert.equal(events[0].event, "transfer");
  assert.equal(events[0].status, "kComplete");
  assert.equal(events[0].target_id, "1");
  assert.equal(events[0].token, "1234");
  assert.equal(events[0].total_bytes, "7");
  assert.equal(events[0].transferred_bytes, "7");
});

function createCases() {
  const cache = join(ROOT, ".cache", "test-env", "nearby-linux-contracts");
  mkdirSync(cache, { recursive: true });
  const root = mkdtempSync(join(cache, "case-"));
  const cases = {
    peerA: join(root, "peer-a"),
    peerB: join(root, "peer-b"),
  };
  for (const directory of Object.values(cases)) {
    mkdirSync(join(directory, "outbound"), { recursive: true });
    mkdirSync(join(directory, "received"), { recursive: true });
  }
  return { cases, root };
}

function fakeContext() {
  let current = 0;
  return {
    now: () => current,
    pollIntervalMs: 1,
    receiverStartDelayMs: 1,
    sleep: (milliseconds) =>
      Promise.resolve().then(() => {
        current += milliseconds;
      }),
    timeoutMs: FAKE_TIMEOUT_MS,
  };
}

function transferLog({ bytes, status, token = "1234", transferred = bytes }) {
  return [
    "QS_EVENT event=transfer status=" +
      `${status} target_id=1 progress=100 transferred_bytes=${transferred} ` +
      `total_bytes=${bytes} token=${token}`,
    "ENCRYPTED_WIFI_LAN",
  ].join("\n");
}

function peerDirectory(cases, peer) {
  if (peer === "peer-a") {
    return cases.peerA;
  }
  return cases.peerB;
}

function oppositePeer(peer) {
  if (peer === "peer-a") {
    return "peer-b";
  }
  return "peer-a";
}

function fakeProcess() {
  const process = {
    logs: () => process.log,
    stop: () => {
      process.stopped = true;
      return Promise.resolve();
    },
    wait: () => {
      process.waited = true;
      return Promise.resolve();
    },
  };
  return process;
}

function receiverCommand(kind, args) {
  if (kind === "connections") {
    return args.includes("--advertise");
  }
  return args.includes("receive");
}

function connectionEvidence(process, receiver) {
  process.log = [
    "OnEndpointFound WIFI_LAN",
    "Connection result kSuccess",
    "SendPayload status=kSuccess",
    "ENCRYPTED_WIFI_LAN",
  ].join("\n");
  receiver.log =
    "Received file\nConnection result kSuccess\nENCRYPTED_WIFI_LAN";
}

function completeTransfer(state, input, process) {
  const argument = input.args.find((value) =>
    value.includes("/cases/outbound/"),
  );
  const payload = basename(argument);
  const receiverPeer = oppositePeer(input.peer);
  const senderDirectory = peerDirectory(state.cases, input.peer);
  const receiverDirectory = peerDirectory(state.cases, receiverPeer);
  const source = join(senderDirectory, "outbound", payload);
  const target = join(receiverDirectory, "received", payload);
  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(source, target);
  const receiver = state.receivers.get(receiverPeer);
  if (state.kind === "connections") {
    connectionEvidence(process, receiver);
    return;
  }
  const bytes = readFileSync(source).byteLength;
  process.log = transferLog({
    bytes,
    status: "kComplete",
    transferred: 0,
  });
  receiver.log = transferLog({
    bytes,
    status: "kComplete",
    transferred: 0,
  });
}

function createTransferRunner(cases, kind) {
  const calls = [];
  const processes = [];
  const receivers = new Map();
  const state = { cases, kind, receivers };
  const runner = {
    start(input) {
      calls.push(input);
      const process = fakeProcess();
      processes.push(process);
      if (receiverCommand(kind, input.args)) {
        receivers.set(input.peer, process);
        if (kind === "connections") {
          process.log = "Received file";
        } else {
          process.log = "";
        }
        return process;
      }
      completeTransfer(state, input, process);
      return process;
    },
  };
  return { calls, processes, runner };
}

function event(fields) {
  return `QS_EVENT ${Object.entries(fields)
    .map(([key, value]) => `${key}=${value}`)
    .join(" ")}`;
}

function actionStatus(action) {
  if (action === "hold") {
    return "held";
  }
  return "kOk";
}

function senderActionEvents(cancelled) {
  if (!cancelled) {
    return [];
  }
  return [
    event({ event: "action-requested", action: "cancel" }),
    event({ event: "action-result", action: "cancel", status: "kOk" }),
  ];
}

function actionLogs(receiver, cancelled, senderPeer) {
  const receiverTerminal = "kReject";
  let senderTerminal = "kCancelled";
  if (!cancelled) {
    senderTerminal = "kReject";
    if (senderPeer === "peer-b") {
      senderTerminal = "kFailed";
    }
  }
  const token = "1234";
  const receiverEvents = [
    event({
      event: "transfer",
      status: "kAwaitingLocalConfirmation",
      token,
    }),
    event({ event: "action-requested", action: receiver.action }),
    event({
      event: "action-result",
      action: receiver.action,
      status: actionStatus(receiver.action),
    }),
  ];
  if (!cancelled) {
    receiverEvents.push(
      event({ event: "transfer", status: receiverTerminal, token }),
    );
  }
  const receiverLog = receiverEvents.join("\n");
  const senderLog = [
    ...senderActionEvents(cancelled),
    event({ event: "transfer", status: senderTerminal, token }),
  ].join("\n");
  return { receiverLog, senderLog };
}

function createActionRunner() {
  const calls = [];
  const processes = [];
  const receivers = new Map();
  const runner = {
    start(input) {
      calls.push(input);
      const process = fakeProcess();
      process.wait = ({ acceptedCodes }) => {
        assert.deepEqual(acceptedCodes, [1]);
        process.waited = true;
        return Promise.resolve();
      };
      processes.push(process);
      if (input.args.includes("receive")) {
        receivers.set(input.peer, process);
        process.action = input.args[input.args.indexOf("--action") + 1];
        process.log = "";
        return process;
      }
      const receiver = receivers.get(oppositePeer(input.peer));
      const cancelled = input.args.includes("cancel");
      const logs = actionLogs(receiver, cancelled, input.peer);
      receiver.log = logs.receiverLog;
      process.log = logs.senderLog;
      return process;
    },
  };
  return { calls, processes, runner };
}

function createStalledActionRunner() {
  const processes = [];
  const runner = {
    start() {
      const process = fakeProcess();
      process.log =
        "QS_EVENT event=transfer token=1234 status=kFailed\n" +
        "connection auth_digits=ABCDE failed";
      processes.push(process);
      return process;
    },
  };
  return { processes, runner };
}

async function runFakeTimers(mock, remaining) {
  if (remaining === 0) {
    return;
  }
  mock.timers.runAll();
  await new Promise(setImmediate);
  await runFakeTimers(mock, remaining - 1);
}

test(CONNECTION_CONTRACT_TEST, async () => {
  const { cases, root } = createCases();
  try {
    const fake = createTransferRunner(cases, "connections");
    const result = await runConnectionsSelfTest({
      cases,
      context: fakeContext(),
      runner: fake.runner,
    });
    assert.deepEqual(
      result.transfers.map(({ direction }) => direction),
      ["a-to-b", "b-to-a"],
    );
    assert.ok(
      result.transfers.every(({ evidence }) =>
        Object.values(evidence).every(Boolean),
      ),
    );
    assert.ok(fake.processes.every((process) => process.stopped));
    assert.deepEqual(
      fake.calls
        .filter(({ args }) => args.includes("--discover"))
        .map(({ peer }) => peer),
      ["peer-a", "peer-b"],
    );
    assert.equal(existsSync(join(cases.peerA, "outbound", "a-to-b")), false);
    assert.equal(existsSync(join(cases.peerB, "received", "a-to-b")), false);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test(SHARING_TRANSFER_TEST, async (context) => {
  const { cases, root } = createCases();
  context.mock.timers.enable({ apis: ["setTimeout"] });
  try {
    const fake = createTransferRunner(cases, "sharing");
    const result = sharingTest.runSharingSelfTest({
      cases,
      runner: fake.runner,
    });
    await runFakeTimers(context.mock, TIMER_TICKS);
    const completed = await result;
    const transfers = [completed.first, completed.second];
    assert.deepEqual(
      transfers.map(({ medium }) => medium),
      ["wifi_lan", "wifi_lan"],
    );
    assert.ok(transfers.every(({ pinMatch }) => pinMatch));
    assert.ok(fake.processes.every((process) => process.stopped));
    assert.deepEqual(
      fake.calls
        .filter(({ args }) => args.includes("send"))
        .map(({ peer }) => peer),
      ["peer-a", "peer-b"],
    );
    assert.ok(
      fake.calls.every(
        ({ variables }) =>
          variables.XDG_CONFIG_HOME === "/run/quickshare/config",
      ),
    );
    assert.ok(
      fake.calls.every(
        ({ variables }) => variables.XDG_STATE_HOME === "/run/quickshare/state",
      ),
    );
  } finally {
    context.mock.timers.reset();
    rmSync(root, { force: true, recursive: true });
  }
});

test(SHARING_ACTION_TEST, async () => {
  const { cases, root } = createCases();
  try {
    const fake = createActionRunner();
    const result = await runSharingActionsSelfTest({
      cases,
      context: fakeContext(),
      runner: fake.runner,
    });
    assert.equal(result.expectedExit, "nonzero");
    assert.deepEqual(
      result.outcomes.map(({ action, direction }) => `${action}:${direction}`),
      ["reject:a-to-b", "reject:b-to-a", "cancel:a-to-b", "cancel:b-to-a"],
    );
    assert.ok(
      result.outcomes.every(
        ({ completedFile, pinMatch }) => !completedFile && pinMatch,
      ),
    );
    assert.deepEqual(
      result.outcomes.map(({ receiverTerminal, senderTerminal }) => ({
        receiverTerminal,
        senderTerminal,
      })),
      [
        { receiverTerminal: "kReject", senderTerminal: "kReject" },
        { receiverTerminal: "kReject", senderTerminal: "kFailed" },
        { receiverTerminal: null, senderTerminal: "kCancelled" },
        { receiverTerminal: null, senderTerminal: "kCancelled" },
      ],
    );
    assert.ok(
      fake.processes.every((process) => process.stopped && process.waited),
    );
    assert.deepEqual(
      fake.calls
        .filter(({ args }) => args.includes("send"))
        .map(({ peer }) => peer),
      ["peer-a", "peer-b", "peer-a", "peer-b"],
    );
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});

test("Sharing action timeouts report only typed peer evidence", async () => {
  const { cases, root } = createCases();
  const fake = createStalledActionRunner();
  try {
    await assert.rejects(
      runSharingActionsSelfTest({
        cases,
        context: fakeContext(),
        runner: fake.runner,
      }),
      (error) => {
        assert.ok(error.message.includes("sender events transfer:failed"));
        assert.ok(error.message.includes("receiver events transfer:failed"));
        assert.ok(!error.message.includes("token"));
        assert.ok(!error.message.includes("auth_digits"));
        assert.ok(!error.message.includes("1234"));
        assert.ok(!error.message.includes("ABCDE"));
        return true;
      },
    );
    assert.ok(fake.processes.every((process) => process.stopped));
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});
