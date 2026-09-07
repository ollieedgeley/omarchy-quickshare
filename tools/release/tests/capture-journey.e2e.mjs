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

function prepare(root) {
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
  symlinkSync(BINARY, join(native, "omarchy-quickshare"));
  const clipboard = join(native, "wl-paste");
  copyFileSync(join(ROOT, "tools/release/tests/clipboard-fake.mjs"), clipboard);
  chmodSync(clipboard, EXECUTABLE_MODE);
  return {
    harness: join(harness, "capture-harness.qml"),
    env: headlessEnvironment(root, {
      PATH: `${native}:${process.env.PATH ?? ""}`,
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

function prepareJourney(root, journey) {
  const { automatic, captureFirst, type } = journey;
  const prepared = prepare(root);
  const file = join(root, "captured A.txt");
  const value = {
    file: `file://${file}`,
    text: "captured A\nexact bytes",
    url: "https://example.test/A?exact=%20&x=1",
  }[type];
  writeFileSync(file, "exact file bytes");
  let attachment = { type, value };
  if (type === "file") {
    attachment = JSON.parse(
      '{"type":"file","name":"captured A.txt","size_bytes":16}',
    );
  }
  prepared.env.CAPTURE_JOURNEY = JSON.stringify({
    attachment,
    automatic,
    captureFirst,
    failReplacement: journey.failReplacement === true,
    replacement: journey.replacement === true,
    value,
  });
  prepared.env.CLIPBOARD_LOG = join(root, "clipboard.log");
  prepared.env.CLIPBOARD_STARTED = join(root, "clipboard.started");
  prepared.env.CLIPBOARD_RELEASE = join(root, "clipboard.release");
  prepared.env.CLIPBOARD_REPLACEMENT = String(journey.replacement === true);
  prepared.env.CLIPBOARD_FAILURE = String(journey.failReplacement === true);
  writeFileSync(prepared.env.CLIPBOARD_LOG, "");
  prepared.env.CLIPBOARD_VALUE = "changed B";
  if (!captureFirst && automatic) {
    prepared.env.CLIPBOARD_VALUE = value;
  }
  return { ...prepared, attachment };
}

function assertJourneyOutcome(prepared, journey) {
  const status = spawnSync(BINARY, ["status", "--json"], {
    env: prepared.env,
    encoding: "utf8",
  });
  assert.equal(status.status, 0, status.stderr);
  const share = JSON.parse(status.stdout).response.snapshot.active_share;
  if (journey.failReplacement) {
    assert.equal(share, null);
  } else {
    assert.deepEqual(share.attachment, prepared.attachment);
    assert.equal(share.peer.id, "pixel-8");
  }
  let expectedReads = "";
  if (!journey.captureFirst && journey.automatic) {
    expectedReads = "read\n";
  }
  if (journey.replacement) {
    expectedReads = "read\nread\n";
  }
  if (journey.failReplacement) {
    expectedReads = "read\nread\nread\n";
  }
  assert.equal(readFileSync(prepared.env.CLIPBOARD_LOG, "utf8"), expectedReads);
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
    const result = spawnSync(
      process.env.QUICKSHELL ?? "quickshell",
      ["--no-color", "-p", prepared.harness],
      { env: prepared.env, encoding: "utf8", timeout: HARNESS_TIMEOUT_MS },
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
