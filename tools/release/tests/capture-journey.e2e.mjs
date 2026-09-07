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
import test from "node:test";
import { setTimeout } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import { HARNESS_STUBS } from "./plugin-harness-stubs.mjs";

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
  writeFileSync(
    clipboard,
    "#!/usr/bin/env node\nprocess.stdout.write('changed B');\n",
  );
  chmodSync(clipboard, EXECUTABLE_MODE);
  return {
    harness: join(harness, "capture-harness.qml"),
    env: {
      ...process.env,
      HOME: root,
      PATH: `${native}:${process.env.PATH ?? ""}`,
      XDG_CONFIG_HOME: join(root, "config"),
      XDG_DATA_HOME: join(root, "data"),
      XDG_RUNTIME_DIR: join(root, "runtime"),
    },
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

test("composed capture-first never rereads changed clipboard", async () => {
  const root = mkdtempSync(join(tmpdir(), "quickshare-capture-"));
  const prepared = prepare(root);
  const binary = BINARY;
  const daemon = spawn(binary, ["daemon", "--simulate"], {
    env: prepared.env,
    stdio: "ignore",
  });
  const exited = once(daemon, "exit");
  try {
    await waitForDaemon(binary, prepared.env, START_ATTEMPTS);
    const setting = spawnSync(
      binary,
      ["config", "set", "read_clipboard_on_select", "true"],
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
    const status = spawnSync(binary, ["status", "--json"], {
      env: prepared.env,
      encoding: "utf8",
    });
    assert.equal(status.status, 0, status.stderr);
    const share = JSON.parse(status.stdout).response.snapshot.active_share;
    assert.equal(share.attachment.value, "captured A\nexact bytes");
    assert.equal(share.peer.id, "pixel-8");
  } finally {
    daemon.kill("SIGTERM");
    await exited;
    rmSync(root, { recursive: true, force: true });
  }
});
