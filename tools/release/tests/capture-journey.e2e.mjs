import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "vitest";
import { fileURLToPath } from "node:url";
import { HARNESS_STUBS, headlessEnvironment } from "./plugin-harness-stubs.mjs";
import {
  observeJourney,
  stopFixtureProcess,
  waitForDaemon,
} from "./capture-operation-fixtures.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const BINARY = resolve(
  process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"),
  "debug/omarchy-quickshare",
);
const DAEMON = resolve(
  process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"),
  "debug/omarchy-quickshare-daemon",
);
const QUICKSHELL = spawnSync(
  "which",
  [process.env.QUICKSHELL ?? "quickshell"],
  { encoding: "utf8" },
).stdout.trim();
const EXECUTABLE_MODE = 0o755;
const START_ATTEMPTS = 100;
const HARNESS_TIMEOUT_MS = 8_000;
const CONNECTION_TIMEOUT_MS = 5_000;
const PRIVATE_DIRECTORY_MODE = 0o700;
const SUCCESS_PATTERN = /HARNESS_OK/u;
const FILES = [
  "BarWidget.qml",
  "QuickShareService.qml",
  "ControlConnection.qml",
  "SharePanel.qml",
  "AttachmentBadge.qml",
  "PeerChoiceView.qml",
  "ConsentView.qml",
  "IdleView.qml",
  "TransferView.qml",
  "TerminalView.qml",
  "release.json",
];

function prepare(root, journey) {
  const harness = join(root, "harness");
  const native = join(root, "bin");
  mkdirSync(harness);
  mkdirSync(native);
  for (const file of FILES) {
    copyFileSync(
      join(ROOT, "packaging/omarchy-plugin", file),
      join(harness, file),
    );
  }
  for (const [file, source] of Object.entries(HARNESS_STUBS)) {
    mkdirSync(dirname(join(harness, file)), { recursive: true });
    writeFileSync(join(harness, file), source);
  }
  copyFileSync(
    join(ROOT, "tools/release/tests/capture-harness.qml"),
    join(harness, "capture-harness.qml"),
  );
  symlinkSync(process.execPath, join(native, "node"));
  for (const utility of ["touch", "rm", "cp"]) {
    const found = spawnSync("which", [utility], { encoding: "utf8" });
    assert.equal(
      found.status,
      0,
      `${utility} is required for capture fixtures`,
    );
    symlinkSync(found.stdout.trim(), join(native, utility));
  }
  const clipboard = join(native, "wl-paste");
  copyFileSync(join(ROOT, "tools/release/tests/clipboard-fake.mjs"), clipboard);
  chmodSync(clipboard, EXECUTABLE_MODE);
  return {
    harness: join(harness, "capture-harness.qml"),
    env: headlessEnvironment(root, {
      PATH: native,
      SUBMISSION_ATTEMPTS: join(root, "submission.attempts"),
      SUBMISSION_ENVELOPES: join(root, "submission.envelopes"),
      SUBMISSION_MODE: journey.submissionMode || "",
      SUBMISSION_RELEASE: join(root, "submission.release"),
      SUBMISSION_STARTED: join(root, "submission.started"),
    }),
  };
}

function prepareClipboard(root, prepared, journey) {
  const { automatic, captureFirst, value } = journey;
  prepared.env.CLIPBOARD_LOG = join(root, "clipboard.log");
  prepared.env.CLIPBOARD_STARTED = join(root, "clipboard.started");
  prepared.env.CLIPBOARD_RELEASE = join(root, "clipboard.release");
  prepared.env.CLIPBOARD_REPLACEMENT = String(
    Boolean(
      journey.replacement ||
      journey.invalidate ||
      journey.peerChange ||
      journey.explicitPending ||
      journey.providedEmpty ||
      journey.readTimeout,
    ),
  );
  prepared.env.CLIPBOARD_FAILURE = String(journey.failReplacement === true);
  writeFileSync(prepared.env.CLIPBOARD_LOG, "");
  prepared.env.CLIPBOARD_VALUE = "changed B";
  if (!captureFirst && automatic && !journey.explicitPending) {
    prepared.env.CLIPBOARD_VALUE = value;
  }
}

function prepareJourney(root, journey) {
  const { type } = journey;
  const prepared = prepare(root, journey);
  const file = join(root, "captured A.txt");
  const value = {
    "existing-path": "captured A.txt",
    file: `file://${file}`,
    flag: "--help",
    text: "captured A\nexact bytes",
    url: "https://example.test/A?exact=%20&x=1",
  }[journey.contentCase ?? type];
  writeFileSync(file, "exact file bytes");
  if (journey.failSubmission) {
    prepared.env.CAPTURE_FILE = file;
    prepared.env.CAPTURE_RESTORE_FILE = join(root, "restore.txt");
    copyFileSync(file, prepared.env.CAPTURE_RESTORE_FILE);
  }
  let attachment = { type, value };
  if (type === "file") {
    attachment = JSON.parse(
      '{"type":"file","name":"captured A.txt","size_bytes":16}',
    );
  }
  let peer = "pixel-8";
  if (journey.recover || journey.peerChange) {
    peer = "galaxy-tab";
  }
  const capture = { ...journey, attachment, peer, value };
  prepared.env.CAPTURE_JOURNEY = JSON.stringify(capture);
  prepareClipboard(root, prepared, capture);
  return { ...prepared, attachment, peer };
}

function startProxy(prepared, daemonEnvironment) {
  const child = spawn(
    process.execPath,
    [join(ROOT, "tools/release/tests/submission-socket-fake.mjs")],
    {
      env: {
        ...prepared.env,
        SUBMISSION_UPSTREAM: join(
          daemonEnvironment.XDG_RUNTIME_DIR,
          "omarchy-quickshare/control.sock",
        ),
      },
      stdio: ["ignore", "ignore", "pipe", "ipc"],
    },
  );
  const proxy = { child, exited: once(child, "exit"), output: "" };
  child.stderr.on("data", (chunk) => {
    proxy.output += chunk;
  });
  return proxy;
}

async function configureDaemon(environment, journey) {
  await waitForDaemon(BINARY, environment, START_ATTEMPTS);
  const setting = spawnSync(
    BINARY,
    ["config", "set", "read_clipboard_on_select", String(journey.automatic)],
    {
      encoding: "utf8",
      env: environment,
      killSignal: "SIGKILL",
      timeout: CONNECTION_TIMEOUT_MS,
    },
  );
  assert.equal(setting.status, 0, setting.stderr);
  if (journey.closedPaste) {
    const pin = spawnSync(BINARY, ["peer", "pin", "pixel-8"], {
      encoding: "utf8",
      env: environment,
      killSignal: "SIGKILL",
      timeout: CONNECTION_TIMEOUT_MS,
    });
    assert.equal(pin.status, 0, pin.stderr);
  }
}

function runPanel(root, prepared, proxy) {
  const result = spawnSync(QUICKSHELL, ["--no-color", "-p", prepared.harness], {
    cwd: root,
    encoding: "utf8",
    env: prepared.env,
    killSignal: "SIGKILL",
    timeout: HARNESS_TIMEOUT_MS,
  });
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  assert.equal(result.status, 0, output + (proxy?.output ?? ""));
  assert.match(output, SUCCESS_PATTERN);
}

async function connectProxy(prepared, environment) {
  const proxy = startProxy(prepared, environment);
  try {
    const [message] = await once(proxy.child, "message", {
      signal: AbortSignal.timeout(CONNECTION_TIMEOUT_MS),
    });
    assert.equal(message, "ready", proxy.output);
    return proxy;
  } catch (error) {
    await stopFixtureProcess(proxy);
    throw error;
  }
}

function isolatedDaemonEnvironment(root, prepared, journey) {
  const environment = { ...prepared.env };
  if (journey.submissionMode) {
    environment.XDG_RUNTIME_DIR = join(root, "daemon-runtime");
    mkdirSync(environment.XDG_RUNTIME_DIR, { mode: PRIVATE_DIRECTORY_MODE });
  }
  return environment;
}

async function runJourney(journey) {
  const root = mkdtempSync(join(tmpdir(), "quickshare-capture-"));
  const prepared = prepareJourney(root, journey);
  const daemonEnvironment = isolatedDaemonEnvironment(root, prepared, journey);
  let proxy = null;
  const child = spawn(DAEMON, ["--simulate"], {
    env: daemonEnvironment,
    stdio: "ignore",
  });
  let daemon = { child, exited: once(child, "exit") };
  try {
    await configureDaemon(daemonEnvironment, journey);
    if (journey.submissionMode) {
      proxy = await connectProxy(prepared, daemonEnvironment);
    }
    daemon = await observeJourney(
      {
        binary: BINARY,
        daemonBinary: DAEMON,
        environment: daemonEnvironment,
        prepared,
        runPanel: () => runPanel(root, prepared, proxy),
      },
      journey,
      daemon,
    );
  } finally {
    try {
      await Promise.all([
        stopFixtureProcess(proxy),
        stopFixtureProcess(daemon),
      ]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
}

for (const type of ["file", "text", "url"]) {
  for (const automatic of [false, true]) {
    for (const captureFirst of [true, false]) {
      const name = `${type}: auto=${automatic}, first=${captureFirst}`;
      test(name, async () => {
        await runJourney({ automatic, captureFirst, type });
      });
    }
  }
}

test("explicit Paste replaces a pending automatic read", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    replacement: true,
    type: "text",
  });
});

test("failed explicit replacement cannot fall back to auto", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    failReplacement: true,
    replacement: true,
    type: "text",
  });
});

for (const invalidate of ["close", "clear", "cancel"]) {
  for (const automatic of [false, true]) {
    test(`${invalidate}: auto=${automatic}`, async () => {
      await runJourney({
        automatic,
        captureFirst: false,
        invalidate,
        type: "text",
      });
    });
  }
}

test("close invalidates an already queued replacement", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    invalidate: "close",
    queuedReplacement: true,
    type: "text",
  });
});

test("fresh Paste and recipient recover after preparation Cancel", async () => {
  await runJourney({
    automatic: false,
    captureFirst: false,
    invalidate: "cancel",
    recover: true,
    type: "text",
  });
});

test("authoritative admission consumes its captured preparation", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    type: "text",
  });
});

for (const newer of ["different", "same"]) {
  test(`admission preserves independent ${newer} capture`, async () => {
    await runJourney({
      automatic: false,
      captureFirst: true,
      newer,
      submissionMode: "after",
      type: "text",
    });
  });
}

test("captured option-like text is sent literally", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    contentCase: "flag",
    type: "text",
  });
});

test("plain text matching a working-directory file stays text", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    contentCase: "existing-path",
    type: "text",
  });
});

test("closed-panel Paste never sends to a preferred peer", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    closedPaste: true,
    type: "text",
  });
});

test("failed dispatch retains capture for deliberate retry", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    failSubmission: true,
    submissionMode: "before",
    type: "file",
  });
});

test("changing recipient during a read captures afresh", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    peerChange: true,
    type: "text",
  });
});

test("pending explicit capture prevents automatic competition", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    explicitPending: true,
    type: "text",
  });
});

test("a stalled clipboard read permits deliberate recovery", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    readTimeout: true,
    type: "text",
  });
});

test("expired automatic read preserves queued explicit capture", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    readTimeout: true,
    timeoutReplacement: true,
    type: "text",
  });
});

test("empty automatic capture waits for deliberate recovery", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    invalidRead: "empty",
    type: "text",
  });
});

test("unsupported clipboard MIME never becomes a text share", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    invalidRead: "unsupported",
    type: "text",
  });
});

test("provided empty value invalidates pending automatic capture", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    providedEmpty: true,
    type: "text",
  });
});

test("lost reply reconciles once; reload respects dismissal", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    submissionMode: "drop",
    type: "text",
  });
});

for (const terminalOrder of [
  "client-only reconnect",
  "cancel-first",
  "complete-first",
]) {
  test(`${terminalOrder}: public terminal ordering`, async () => {
    await runJourney({
      automatic: false,
      captureFirst: true,
      clientReconnect: true,
      submissionMode: "client-reconnect",
      terminalOrder,
      type: "text",
    });
  });
}
