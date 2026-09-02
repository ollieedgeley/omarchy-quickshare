import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  accessSync,
  closeSync,
  constants,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";

import {
  androidEnvironment,
  commandPaths,
  environmentPaths,
} from "./paths.mjs";

const ADB_PROBE_TIMEOUT_MILLISECONDS = 2_000;
const BOOT_TIMEOUT_MILLISECONDS = 180_000;
const COLD_BOOT_TIMEOUT_MILLISECONDS = 180_000;
const EXIT_TIMEOUT_MILLISECONDS = 30_000;
const POLL_MILLISECONDS = 250;
const PROBE_PACKAGE = "dev.omarchy.quickshare.probe";

export function deviceSerial(peer) {
  return `emulator-${peer.consolePort}`;
}

export function emulatorArguments(peer, commands, avds) {
  return [
    `@${peer.name}`,
    "-port",
    String(peer.consolePort),
    "-no-window",
    "-no-audio",
    "-no-boot-anim",
    "-no-metrics",
    "-memory",
    String(avds.memoryMegabytes),
    "-adb-path",
    commands.adb,
    "-skip-adb-auth",
    "-accel",
    "on",
    "-gpu",
    "swiftshader_indirect",
    "-netdelay",
    "none",
    "-netspeed",
    "full",
  ];
}

export function coldEmulatorArguments(peer, commands, avds) {
  return [
    ...emulatorArguments(peer, commands, avds),
    "-no-snapshot-load",
    "-wipe-data",
  ];
}

function context(manifest) {
  const paths = environmentPaths();
  const commands = commandPaths(paths, manifest);
  const environment = androidEnvironment(paths, commands, manifest);
  return { commands, environment, paths };
}

function adbKeyPaths(paths) {
  const privateKey = join(paths.emulatorHome, "adbkey");
  return { privateKey, publicKey: `${privateKey}.pub` };
}

function ensureAdbKey(current) {
  const keys = adbKeyPaths(current.paths);
  mkdirSync(current.paths.emulatorHome, { recursive: true });
  if (existsSync(keys.privateKey) !== existsSync(keys.publicKey)) {
    rmSync(keys.privateKey, { force: true });
    rmSync(keys.publicKey, { force: true });
  }
  if (!existsSync(keys.privateKey)) {
    const result = spawnSync(
      current.commands.adb,
      ["keygen", keys.privateKey],
      { encoding: "utf8", env: current.environment, stdio: "pipe" },
    );
    if (result.status !== 0) {
      throw new Error(`ADB key generation failed: ${result.stderr}`);
    }
  }
  accessSync(keys.privateKey, constants.R_OK);
  accessSync(keys.publicKey, constants.R_OK);
}

function seedMarker(paths) {
  return join(paths.state, "seed-key.sha256");
}

function seedFingerprint(manifest, paths) {
  const { publicKey } = adbKeyPaths(paths);
  return createHash("sha256")
    .update(readFileSync(publicKey))
    .update(JSON.stringify(manifest.avds))
    .digest("hex");
}

function assertSeeded(manifest, current) {
  const marker = seedMarker(current.paths);
  if (
    !existsSync(marker) ||
    readFileSync(marker, "utf8").trim() !==
      seedFingerprint(manifest, current.paths)
  ) {
    throw new Error("Android AVDs need seeding; run `make android-seed`");
  }
}

function startAdbServer(current) {
  const result = spawnSync(current.commands.adb, ["start-server"], {
    encoding: "utf8",
    env: current.environment,
    stdio: "pipe",
  });
  if (result.status !== 0) {
    throw new Error(`Dedicated ADB server failed: ${result.stderr}`);
  }
}

function stopAdbServer(current) {
  spawnSync(current.commands.adb, ["kill-server"], {
    env: current.environment,
    stdio: "ignore",
  });
}

function probeApk(paths) {
  return join(
    paths.probeBuild,
    "app",
    "outputs",
    "apk",
    "debug",
    "app-debug.apk",
  );
}

function probeMarker(paths, peer) {
  return join(paths.state, `${peer.name}.probe.sha256`);
}

function probeFingerprint(paths) {
  return createHash("sha256")
    .update(readFileSync(probeApk(paths)))
    .digest("hex");
}

function pidPath(paths, peer) {
  return join(paths.state, `${peer.name}.pid`);
}

function assertPrepared(manifest, current) {
  const executables = [current.commands.adb, current.commands.emulator];
  for (const executable of executables) {
    accessSync(executable, constants.X_OK);
  }
  accessSync(probeApk(current.paths), constants.R_OK);
  for (const peer of manifest.avds.peers) {
    const config = join(
      current.paths.avdHome,
      `${peer.name}.avd`,
      "config.ini",
    );
    accessSync(config, constants.R_OK);
  }
}

function startPeer(current, peer, launch) {
  const logPath = join(current.paths.diagnostics, `${peer.name}.emulator.log`);
  const log = openSync(logPath, "w");
  const child = spawn(current.commands.emulator, launch.arguments(peer), {
    detached: true,
    env: launch.environment,
    stdio: ["ignore", log, log],
  });
  closeSync(log);
  if (typeof child.pid !== "number") {
    throw new Error(`Android emulator ${peer.name} did not start`);
  }
  writeFileSync(pidPath(current.paths, peer), `${child.pid}\n`);
  child.unref();
}

function adb(current, peer, options) {
  const timeout = options.timeout ?? BOOT_TIMEOUT_MILLISECONDS;
  return spawnSync(
    current.commands.adb,
    ["-s", deviceSerial(peer), ...options.argumentsList],
    {
      encoding: "utf8",
      env: current.environment,
      stdio: "pipe",
      timeout,
    },
  );
}

function booted(current, peer) {
  const result = adb(current, peer, {
    argumentsList: ["shell", "getprop", "sys.boot_completed"],
    timeout: ADB_PROBE_TIMEOUT_MILLISECONDS,
  });
  return result.status === 0 && result.stdout.trim() === "1";
}

function delay() {
  return new Promise((resolveDelay) => {
    setTimeout(resolveDelay, POLL_MILLISECONDS);
  });
}

async function waitForBoot(current, peer, deadline) {
  if (Date.now() >= deadline) {
    throw new Error(`Android emulator ${peer.name} missed its boot deadline`);
  }
  if (booted(current, peer)) {
    return;
  }
  await delay();
  await waitForBoot(current, peer, deadline);
}

function installProbe(current, peer) {
  const fingerprint = probeFingerprint(current.paths);
  const marker = probeMarker(current.paths, peer);
  const installed = adb(current, peer, {
    argumentsList: ["shell", "pm", "path", PROBE_PACKAGE],
  });
  if (
    installed.status === 0 &&
    existsSync(marker) &&
    readFileSync(marker, "utf8").trim() === fingerprint
  ) {
    return;
  }
  adb(current, peer, { argumentsList: ["uninstall", PROBE_PACKAGE] });
  const result = adb(current, peer, {
    argumentsList: ["install", "-g", probeApk(current.paths)],
  });
  if (result.status !== 0) {
    throw new Error(
      `Probe installation failed on ${peer.name}: ` +
        `${result.stdout}${result.stderr}`,
    );
  }
  writeFileSync(marker, `${fingerprint}\n`);
}

function readTrackedPid(paths, peer) {
  const path = pidPath(paths, peer);
  if (!existsSync(path)) {
    return null;
  }
  const pid = Number.parseInt(readFileSync(path, "utf8"), 10);
  if (Number.isSafeInteger(pid) && pid > 1) {
    return pid;
  }
  return null;
}

function processMatchesPeer(pid, peer) {
  try {
    const command = readFileSync(`/proc/${pid}/cmdline`, "utf8");
    return command.includes("emulator") && command.includes(`@${peer.name}`);
  } catch {
    return false;
  }
}

async function stopPeer(current, peer, deadline) {
  const path = pidPath(current.paths, peer);
  const pid = readTrackedPid(current.paths, peer);
  if (pid === null || !processMatchesPeer(pid, peer)) {
    rmSync(path, { force: true });
    return;
  }
  if (Date.now() < deadline) {
    await delay();
    await stopPeer(current, peer, deadline);
    return;
  }
  process.kill(pid, "SIGTERM");
  rmSync(path, { force: true });
}

export async function down(manifest) {
  const startedAt = Date.now();
  const current = context(manifest);
  for (const peer of manifest.avds.peers) {
    adb(current, peer, { argumentsList: ["emu", "kill"] });
  }
  const deadline = startedAt + EXIT_TIMEOUT_MILLISECONDS;
  await Promise.all(
    manifest.avds.peers.map((peer) => stopPeer(current, peer, deadline)),
  );
  stopAdbServer(current);
  process.stdout.write(
    `Android AVD lab stopped in ${Date.now() - startedAt}ms.\n`,
  );
}

async function startLab(manifest, profile) {
  const startedAt = Date.now();
  const current = context(manifest);
  const peers = profile.peers ?? manifest.avds.peers;
  assertPrepared(manifest, current);
  mkdirSync(current.paths.diagnostics, { recursive: true });
  mkdirSync(current.paths.state, { recursive: true });
  ensureAdbKey(current);
  if (profile.requiresSeed) {
    assertSeeded(manifest, current);
  }
  startAdbServer(current);
  const launch = {
    arguments: (peer) =>
      profile.arguments(peer, current.commands, manifest.avds),
    environment: current.environment,
  };
  for (const peer of peers) {
    startPeer(current, peer, launch);
  }
  try {
    const deadline = startedAt + profile.timeout;
    await Promise.all(
      peers.map((peer) => waitForBoot(current, peer, deadline)),
    );
    for (const peer of peers) {
      installProbe(current, peer);
    }
  } catch (error) {
    await down(manifest);
    throw error;
  }
  process.stdout.write(
    `Android AVD lab ready in ${Date.now() - startedAt}ms.\n`,
  );
}

export async function up(manifest) {
  await startLab(manifest, {
    arguments: emulatorArguments,
    requiresSeed: true,
    timeout: BOOT_TIMEOUT_MILLISECONDS,
  });
}

async function seedPeers(manifest, peers) {
  const [peer, ...remaining] = peers;
  if (!peer) {
    return;
  }
  await startLab(manifest, {
    arguments: coldEmulatorArguments,
    peers: [peer],
    requiresSeed: false,
    timeout: COLD_BOOT_TIMEOUT_MILLISECONDS,
  });
  await down(manifest);
  await seedPeers(manifest, remaining);
}

export async function seed(manifest) {
  const current = context(manifest);
  mkdirSync(current.paths.state, { recursive: true });
  rmSync(seedMarker(current.paths), { force: true });
  await seedPeers(manifest, manifest.avds.peers);
  writeFileSync(
    seedMarker(current.paths),
    `${seedFingerprint(manifest, current.paths)}\n`,
  );
}
