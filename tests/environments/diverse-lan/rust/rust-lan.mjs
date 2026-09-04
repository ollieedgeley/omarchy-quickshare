import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { createComposeRunner } from "../../nearby-linux/compose-runner.mjs";

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

async function waitFor(directories, description, predicate) {
  const snapshot = await status(directories).catch((error) => {
    directories.statusError = error;
    return null;
  });
  if (snapshot && predicate(snapshot)) {
    return snapshot;
  }
  if (snapshot) {
    recordLastSnapshot(directories, snapshot);
  }
  if (Date.now() >= directories.discoveryDeadline) {
    const evidence = JSON.stringify(
      directories.lastSnapshot ?? directories.statusError?.message,
    );
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

function startGoogleReceiver(directories) {
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
      "accept",
      "--name",
      "Google-B",
      "--timeout",
      "18",
    ],
    peer: "google",
    variables: {
      QUICKSHARE_PIN_SALT: "rust-lan",
      XDG_CONFIG_HOME: "/run/quickshare/config",
      XDG_DOWNLOAD_DIR: "/cases/received",
      XDG_RUNTIME_DIR: "/run/quickshare",
      XDG_STATE_HOME: "/run/quickshare/state",
    },
  });
}

function startGoogleSender(directories, file) {
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
      "--name",
      "Google-A",
      "--timeout",
      "18",
    ],
    peer: "google",
    variables: {
      QUICKSHARE_PIN_SALT: "rust-lan",
      XDG_CONFIG_HOME: "/run/quickshare/config",
      XDG_DOWNLOAD_DIR: "/cases/received",
      XDG_RUNTIME_DIR: "/run/quickshare",
      XDG_STATE_HOME: "/run/quickshare/state",
    },
  });
}

async function assertGoogleToRust(directories) {
  const file = `google-to-rust-${randomUUID()}.txt`;
  const source = join(directories.google, "outbound", file);
  writeFileSync(source, Buffer.alloc(MULTI_FRAME_FILE_SIZE, "G"));
  await rustCommand(["visibility", "open"], directories);
  const sender = startGoogleSender(directories, file);
  try {
    const offered = await waitFor(
      directories,
      "report the inbound file offer",
      (value) => value.active_share?.direction === "inbound",
    );
    const shareId = String(offered.active_share.id);
    await rustCommand(["share", "accept", shareId], directories);
    await sender.wait({ timeoutMs: DISCOVERY_TIMEOUT_MS });
    resetDiscoveryDeadline(directories);
    await waitFor(
      directories,
      "complete the inbound share",
      (value) => value.active_share?.phase === "completed",
    );
    const received = join(
      directories.rust,
      "received",
      RECEIVED_SUBDIRECTORY,
      file,
    );
    assert.equal(transferHash(source), transferHash(received));
  } catch (error) {
    throw new Error(
      `Rust daemon log:\n${directories.daemonLogs()}\n` +
        `Reference peer log:\n${sender.logs()}`,
      { cause: error },
    );
  } finally {
    await sender.stop();
  }
}

async function assertRustToGoogle(directories) {
  const file = `rust-to-google-${randomUUID()}.txt`;
  const source = join(directories.rust, "outbound", file);
  writeFileSync(source, Buffer.alloc(MULTI_FRAME_FILE_SIZE, "R"));
  await rustCommand(["discover", "start"], directories);
  const receiver = startGoogleReceiver(directories);
  try {
    const snapshot = await waitFor(
      directories,
      "report the reference mDNS peer",
      (value) => value.peers?.length,
    );
    const queued = await rustCommand(
      ["send", `/cases/outbound/${file}`],
      directories,
    );
    const shareId = queued.match(QUEUED_SHARE_PATTERN)?.groups.id;
    assert.ok(
      shareId,
      `Rust daemon did not queue an outbound share: ${queued}`,
    );
    await rustCommand(
      ["share", "select", shareId, peerId(snapshot)],
      directories,
    );
    await receiver.wait({ timeoutMs: DISCOVERY_TIMEOUT_MS });
    resetDiscoveryDeadline(directories);
    await waitFor(
      directories,
      "complete the outbound share",
      (value) =>
        String(value.active_share?.id) === shareId &&
        value.active_share?.phase === "completed",
    );
    const received = join(directories.google, "received", file);
    assert.equal(transferHash(source), transferHash(received));
  } catch (error) {
    throw new Error(
      `Rust daemon log:\n${directories.daemonLogs()}\n` +
        `Reference peer log:\n${receiver.logs()}`,
      { cause: error },
    );
  } finally {
    await receiver.stop();
  }
}

function startDaemon(directories) {
  const daemon = spawn(
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
    ],
    { cwd: ROOT, env: environment(directories) },
  );
  let logs = "";
  daemon.stdout.on("data", (chunk) => {
    logs += chunk;
  });
  daemon.stderr.on("data", (chunk) => {
    logs += chunk;
  });
  return { daemon, logs: () => logs };
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

export async function runRustLanScenario({ direction }) {
  assert.ok(direction === "rust-to-google" || direction === "google-to-rust");
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
      await assertRustToGoogle(directories);
    } else {
      await assertGoogleToRust(directories);
    }
  } catch (error) {
    failure = error;
  } finally {
    daemon?.daemon.kill();
    await compose(
      ["down", "--remove-orphans", "--volumes"],
      directories,
      false,
    );
    try {
      removeCaseRoot(directories.root);
    } catch (error) {
      failure ??= error;
    }
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
