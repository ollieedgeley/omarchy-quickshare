import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { createServer } from "node:net";
import { delimiter, isAbsolute, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { headlessEnvironment } from "./plugin-harness-stubs.mjs";

const FRAME_LIMIT = 1_048_576;
const PROCESS_TIMEOUT = 25_000;
const FRAGMENT_DELAY = 30;
const ACTION_DELAY = 150;
const TRICKLE_INTERVAL = 200;
const EXACT_ID = "18446744073709551615";
const PEER_NAME = "Café 東京 📱";
const CONTROL_PROTOCOL = 7;
const UTF8_ACCENT_BYTES = 2;
const EXACT_ID_PATTERN = new RegExp(`"share_id":${EXACT_ID}[,}]`, "u");
const SUCCESS_PATTERN = /HARNESS_OK/u;
const INVALID_UTF8_BYTE = 0xff;
const REQUEST_ID_LIMIT = 128;
const HISTORY_COUNT = 2;
const PREVIOUS_REQUEST = -2;
const MODES = [
  "ready",
  "recovery",
  "version",
  "eof",
  "overflow",
  "deadline",
  "boundary",
  "invalid-utf8",
  "lost-submit",
  "unknown-submit",
  "queued-submit",
  "invalid-submit",
];

function operation(requestId, overrides = {}) {
  return {
    ...JSON.parse(
      `{"direction":"outbound","dismissed":false,"id_string":"${EXACT_ID}",` +
        '"phase":"completed","recovery_guidance":null,' +
        `"request_id":${JSON.stringify(requestId)},` +
        '"terminal_reason":null,"total_bytes":12,"transferred_bytes":12}',
    ),
    ...overrides,
  };
}

function snapshot(name = PEER_NAME) {
  const peerName = JSON.stringify(name);
  return (
    `{"version":7,"response":{"type":"snapshot","snapshot":{` +
    `"peers":[{"id":"unicode-peer","name":${peerName}}],` +
    `"active_share":{"id":7,"id_string":"${EXACT_ID}",` +
    `"phase":"awaiting_local_consent","medium":"wifi_lan"},` +
    `"visibility":"closed","visibility_status":{"discoverable":false,` +
    `"requested":false,"temporary":false,"remaining_secs":null,` +
    `"available_media":[],"error":null}},` +
    `"operations":[],` +
    `"preferences":{"saved":{"device_name":${peerName}},` +
    `"applied":{"device_name":${peerName}},` +
    `"pending":false,"error":null}}}`
  );
}

function prepareHarness(root, pluginSource) {
  const directory = join(root, "harness");
  mkdirSync(directory);
  for (const file of [
    "QuickShareService.qml",
    "ControlConnection.qml",
    "release.json",
  ]) {
    copyFileSync(join(pluginSource, file), join(directory, file));
  }
  const harness = join(directory, "status-harness.qml");
  copyFileSync(
    fileURLToPath(new URL("./status-harness.qml", import.meta.url)),
    harness,
  );
  return harness;
}

function executablePath(quickshell) {
  if (isAbsolute(quickshell)) {
    return quickshell;
  }
  if (quickshell.includes("/")) {
    return resolve(quickshell);
  }
  const executable = (process.env.PATH ?? "")
    .split(delimiter)
    .map((path) => join(path, quickshell))
    .find((path) => existsSync(path));
  assert.ok(executable, `Quickshell executable not found: ${quickshell}`);
  return executable;
}

async function replyBoundary(socket) {
  const prefix = `${snapshot().slice(0, -1)},"padding":"`;
  const suffix = 'é"}';
  const padding = FRAME_LIMIT - Buffer.byteLength(prefix + suffix);
  const frame = Buffer.from(`${prefix + " ".repeat(padding) + suffix}\n`);
  assert.equal(frame.length, FRAME_LIMIT + 1);
  const split = frame.lastIndexOf(Buffer.from("é")) + 1;
  socket.write(frame.subarray(0, split));
  await delay(FRAGMENT_DELAY);
  socket.end(frame.subarray(split));
}

async function replySnapshot(socket, mode) {
  if (mode === "version") {
    socket.end(`${snapshot().replace('"version":7', '"version":6')}\n`);
  } else if (mode === "eof") {
    socket.end(snapshot());
  } else if (mode === "overflow") {
    // UTF-8 bytes exceed the limit despite fewer characters.
    socket.end(`"${"é".repeat(FRAME_LIMIT / UTF8_ACCENT_BYTES)}"\n`);
  } else if (mode === "boundary") {
    await replyBoundary(socket);
  } else if (mode === "invalid-utf8") {
    const frame = Buffer.from(`${snapshot("invalid-name")}\n`);
    frame[frame.indexOf("invalid-name")] = INVALID_UTF8_BYTE;
    socket.end(frame);
  } else if (mode === "deadline") {
    socket.write('{"version":7,');
    const trickle = setInterval(() => socket.write(" "), TRICKLE_INTERVAL);
    socket.once("close", () => clearInterval(trickle));
  } else {
    let name = PEER_NAME;
    if (mode === "recovery") {
      name = "Recovered 📱";
    }
    const frame = Buffer.from(`${snapshot(name)}\n`);
    const split = frame.indexOf(Buffer.from("📱")) + 1;
    socket.write(frame.subarray(0, split));
    await delay(FRAGMENT_DELAY);
    socket.write(frame.subarray(split, split + 1));
    await delay(FRAGMENT_DELAY);
    socket.end(frame.subarray(split + 1));
  }
}

function acceptSubmission(socket, mode, { envelope, state }) {
  assert.equal(state.requestId, null, "submission was resent");
  assert.deepEqual(Object.keys(envelope).sort(), [
    "request",
    "request_id",
    "version",
  ]);
  const requestId = envelope.request_id;
  assert.equal(typeof requestId, "string");
  assert.ok(Buffer.byteLength(requestId) > 0);
  assert.ok(Buffer.byteLength(requestId) <= REQUEST_ID_LIMIT);
  assert.deepEqual(
    envelope.request,
    JSON.parse(
      '{"peer_id":"unicode-peer","text":"socket payload","type":"submit_text"}',
    ),
  );
  state.requestId = requestId;
  let overrides = {};
  if (mode === "queued-submit") {
    overrides = JSON.parse(
      '{"phase":"waiting_for_peer","transferred_bytes":0}',
    );
  }
  state.operations.push(operation(requestId, overrides));
  if (mode === "queued-submit" || mode === "invalid-submit") {
    let accepted = state.operations.at(-1);
    if (mode === "invalid-submit") {
      accepted = operation("wrong-request");
    }
    socket.end(
      `${JSON.stringify({
        response: { operation: accepted, type: "queued" },
        version: CONTROL_PROTOCOL,
      })}\n`,
    );
  } else {
    socket.end();
  }
}

async function replyOperation(socket, mode, { envelope, state }) {
  assert.deepEqual(
    envelope.request,
    JSON.parse(
      `{"request_id":${JSON.stringify(state.requestId)},"type":"operation"}`,
    ),
  );
  assert.equal(
    state.requests.at(PREVIOUS_REQUEST).request.type,
    "snapshot",
    "reconciliation did not refresh the snapshot first",
  );
  await delay(ACTION_DELAY);
  let response = { operation: state.operations.at(-1), type: "operation" };
  if (mode === "unknown-submit") {
    response = {
      code: "outcome_unknown",
      message: "No retained request outcome",
      type: "error",
    };
  }
  socket.end(`${JSON.stringify({ response, version: CONTROL_PROTOCOL })}\n`);
}

async function replyJourney(socket, mode, { envelope, request, state }) {
  const { type } = envelope.request;
  if (type === "snapshot") {
    assert.deepEqual(envelope.request, { type: "snapshot" });
    const frame = JSON.parse(snapshot());
    Object.assign(frame.response.snapshot, JSON.parse('{"active_share":null}'));
    frame.response.operations = state.operations;
    socket.end(`${JSON.stringify(frame)}\n`);
  } else if (type === "submit_text") {
    acceptSubmission(socket, mode, { envelope, state });
  } else if (type === "operation") {
    await replyOperation(socket, mode, { envelope, state });
  } else {
    assert.equal(type, "dismiss");
    assert.equal(mode, "lost-submit");
    assert.match(request, EXACT_ID_PATTERN);
    assert.deepEqual(Object.keys(envelope.request).sort(), [
      "share_id",
      "type",
    ]);
    state.operations.at(-1).dismissed = true;
    socket.end('{"version":7,"response":{"type":"applied"}}\n');
  }
}

async function reply(socket, mode, { actions, request, state }) {
  const envelope = JSON.parse(request);
  assert.equal(envelope.version, CONTROL_PROTOCOL);
  state.requests.push(envelope);
  const { type } = envelope.request;
  if (!type.startsWith("submit_")) {
    assert.equal(Object.hasOwn(envelope, "request_id"), false);
    assert.deepEqual(Object.keys(envelope).sort(), ["request", "version"]);
  }
  if (mode.endsWith("-submit")) {
    await replyJourney(socket, mode, { envelope, request, state });
    return;
  }
  if (envelope.request.type === "snapshot") {
    await replySnapshot(socket, mode);
    return;
  }
  actions.push(request.trim());
  assert.match(request, EXACT_ID_PATTERN);
  if (envelope.request.type === "cancel") {
    socket.end();
    return;
  }
  assert.equal(envelope.request.type, "accept");
  await delay(ACTION_DELAY);
  socket.end('{"version":7,"response":{"type":"applied"}}\n');
}

function createFixtureServer(mode, { actions, errors, sockets, state }) {
  return createServer((socket) => {
    sockets.add(socket);
    socket.on("error", () => {
      // Protocol rejection can close the client while a reply is being written.
    });
    socket.on("close", () => sockets.delete(socket));
    let request = "";
    socket.on("data", (chunk) => {
      request += chunk.toString("utf8");
      if (!request.includes("\n")) {
        return;
      }
      socket.removeAllListeners("data");
      reply(socket, mode, { actions, request, state }).catch((error) => {
        errors.push(error);
        socket.destroy();
      });
    });
  });
}

async function runQuickshell(executable, harness, root) {
  const child = spawn(executable, ["--no-color", "-p", harness], {
    env: headlessEnvironment(root, {
      PATH: "",
      QUICKSHARE_SOCKET_ROOT: root,
    }),
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.setEncoding("utf8").on("data", (chunk) => {
    output += chunk;
  });
  child.stderr.setEncoding("utf8").on("data", (chunk) => {
    output += chunk;
  });
  const watchdog = setTimeout(() => child.kill("SIGKILL"), PROCESS_TIMEOUT);
  try {
    const [code] = await once(child, "close");
    assert.equal(code, 0, output);
    assert.match(output, SUCCESS_PATTERN);
  } finally {
    clearTimeout(watchdog);
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      await once(child, "close");
    }
  }
}

function assertJourneys(states) {
  const ids = new Set();
  for (const mode of MODES.filter((value) => value.endsWith("-submit"))) {
    const state = states[MODES.indexOf(mode)];
    const mutations = state.requests.filter(
      ({ request }) =>
        request.type !== "snapshot" && request.type !== "operation",
    );
    const expectedMutations = ["submit_text"];
    if (mode === "lost-submit") {
      expectedMutations.push("dismiss");
    }
    assert.deepEqual(
      mutations.map(({ request }) => request.type),
      expectedMutations,
    );
    assert.ok(
      !ids.has(state.requestId),
      "request identity reused across services",
    );
    ids.add(state.requestId);
    const queries = state.requests.filter(
      ({ request }) => request.type === "operation",
    );
    if (mode === "unknown-submit") {
      assert.ok(queries.length >= HISTORY_COUNT);
    } else if (mode === "queued-submit") {
      assert.equal(queries.length, 0);
    } else {
      assert.equal(queries.length, 1);
    }
    if (mode === "lost-submit") {
      assert.equal(
        state.operations.length,
        HISTORY_COUNT,
        "dismiss removed durable history",
      );
      assert.equal(state.operations[0].dismissed, false);
      assert.equal(state.operations[1].dismissed, true);
    }
  }
}

function initialState() {
  return {
    operations: [
      operation(
        "historical-request",
        JSON.parse(
          '{"id_string":"7","phase":"failed",' +
            '"recovery_guidance":"Retry explicitly",' +
            '"terminal_reason":"interrupted","transferred_bytes":0}',
        ),
      ),
    ],
    requestId: null,
    requests: [],
  };
}

export async function runSocketHarness({ root, pluginSource, quickshell }) {
  const harness = prepareHarness(root, pluginSource);
  const executable = executablePath(quickshell);
  const sockets = new Set();
  const actions = [];
  const errors = [];
  const states = MODES.map(initialState);
  const servers = MODES.map((mode, index) =>
    createFixtureServer(mode, {
      actions,
      errors,
      sockets,
      state: states[index],
    }),
  );
  try {
    await Promise.all(
      servers.map((server, index) => {
        server.listen(join(root, `${MODES[index]}.sock`));
        return once(server, "listening");
      }),
    );
    await runQuickshell(executable, harness, root);
    assert.deepEqual(errors, []);
    assert.deepEqual(
      actions.map((line) => JSON.parse(line).request.type),
      ["accept", "cancel"],
    );
    assertJourneys(states);
  } finally {
    for (const socket of sockets) {
      socket.destroy();
    }
    await Promise.all(
      servers.map(
        (server) =>
          new Promise((done) => {
            server.close(done);
          }),
      ),
    );
  }
}
