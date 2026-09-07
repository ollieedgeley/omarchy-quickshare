import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { setTimeout } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { HARNESS_STUBS, headlessEnvironment } from "./plugin-harness-stubs.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const BINARY = resolve(
  process.env.CARGO_TARGET_DIR ?? join(ROOT, "target"),
  "debug/omarchy-quickshare",
);
const EXECUTABLE_MODE = 0o755;
const START_ATTEMPTS = 100;
const RETRY_MS = 10;
const HARNESS_TIMEOUT_MS = 8_000;
const SUCCESS_PATTERN = /HARNESS_OK/u;
const FILES = [
  "BarWidget.qml",
  "StatusProbe.qml",
  "SharePanel.qml",
  "AttachmentBadge.qml",
  "PeerChoiceView.qml",
  "ConsentView.qml",
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
  const executable = join(native, "omarchy-quickshare");
  if (journey.submissionMode) {
    copyFileSync(
      join(ROOT, "tools/release/tests/submission-process-fake.mjs"),
      executable,
    );
    chmodSync(executable, EXECUTABLE_MODE);
  } else {
    symlinkSync(BINARY, executable);
  }
  const clipboard = join(native, "wl-paste");
  copyFileSync(join(ROOT, "tools/release/tests/clipboard-fake.mjs"), clipboard);
  chmodSync(clipboard, EXECUTABLE_MODE);
  return {
    harness: join(harness, "capture-harness.qml"),
    env: headlessEnvironment(root, {
      PATH: `${native}:${process.env.PATH ?? ""}`,
      QUICKSHARE_REAL_BINARY: BINARY,
      SUBMISSION_ATTEMPTS: join(root, "submission.attempts"),
      SUBMISSION_MODE: journey.submissionMode || "",
      SUBMISSION_RELEASE: join(root, "submission.release"),
      SUBMISSION_STARTED: join(root, "submission.started"),
    }),
  };
}

async function waitForDaemon(binary, environment, attempts) {
  const result = spawnSync(binary, ["health"], { env: environment });
  if (result.status === 0) {
    return;
  }
  assert.ok(attempts > 0, "isolated simulated daemon did not become ready");
  await setTimeout(RETRY_MS);
  await waitForDaemon(binary, environment, attempts - 1);
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
      journey.explicitFailure ||
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

function assertJourneyOutcome(prepared, journey) {
  const status = spawnSync(BINARY, ["status", "--json"], {
    env: prepared.env,
    encoding: "utf8",
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
    journey.invalidRead ||
    journey.explicitFailure
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
      "send\nsend\n",
    );
  }
}

async function runJourney(journey) {
  const root = mkdtempSync(join(tmpdir(), "quickshare-capture-"));
  const prepared = prepareJourney(root, journey);
  const { automatic } = journey;
  const daemon = spawn(BINARY, ["daemon", "--simulate"], {
    env: prepared.env,
    stdio: "ignore",
  });
  const exited = once(daemon, "exit");
  try {
    await waitForDaemon(BINARY, prepared.env, START_ATTEMPTS);
    const setting = spawnSync(
      BINARY,
      ["config", "set", "read_clipboard_on_select", String(automatic)],
      { env: prepared.env, encoding: "utf8" },
    );
    assert.equal(setting.status, 0, setting.stderr);
    if (journey.closedPaste) {
      const pin = spawnSync(BINARY, ["peer", "pin", "pixel-8"], {
        env: prepared.env,
        encoding: "utf8",
      });
      assert.equal(pin.status, 0, pin.stderr);
    }
    const result = spawnSync(
      process.env.QUICKSHELL ?? "quickshell",
      ["--no-color", "-p", prepared.harness],
      {
        cwd: root,
        encoding: "utf8",
        env: prepared.env,
        timeout: HARNESS_TIMEOUT_MS,
      },
    );
    const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
    assert.equal(result.status, 0, output);
    assert.match(output, SUCCESS_PATTERN);
    assertJourneyOutcome(prepared, journey);
  } finally {
    daemon.kill("SIGTERM");
    await exited;
    rmSync(root, { recursive: true, force: true });
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

for (const automatic of [false, true]) {
  for (const explicitFailure of ["empty", "unsupported", "failed"]) {
    test(`A survives ${explicitFailure}, auto=${automatic}`, async () => {
      await runJourney({
        automatic,
        captureFirst: true,
        consume: true,
        explicitFailure,
        type: "text",
      });
    });
  }
}
