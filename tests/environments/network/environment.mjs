import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const NETWORK_CACHE = join(CACHE, "network");
const SOURCE = join(CACHE, "sources", "trees", "wmediumd");
const WORKSPACE = join(NETWORK_CACHE, "wmediumd");
const ARTIFACTS = join(NETWORK_CACHE, "artifacts");
const BINARY = join(ARTIFACTS, "wmediumd");
const MEDIUM_CONTAINER = "omarchy-quickshare-wmediumd";
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.toolchain");

function run(command, args, options = {}) {
  if (!options.quiet) process.stdout.write(`+ ${command} ${args.join(" ")}\n`);
  const result = spawnSync(command, args, {
    encoding: "utf8",
    env: options.env ?? process.env,
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0 && !options.allowFailure) {
    const detail = options.capture
      ? `\n${result.stdout ?? ""}${result.stderr ?? ""}`
      : "";
    throw new Error(`${command} exited with ${result.status}${detail}`);
  }
  return result;
}

function output(command, args) {
  return run(command, args, { capture: true, quiet: true }).stdout.trim();
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function validateEnvironment(manifestSource, dockerfile) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) throw new Error("unsupported network schema");
  if (!/^debian@sha256:[0-9a-f]{64}$/.test(manifest.base)) {
    throw new Error("network base image must use a SHA-256 digest");
  }
  if (!/^\d{8}T\d{6}Z$/.test(manifest.debianSnapshot)) {
    throw new Error("network Debian snapshot must be timestamped");
  }
  if (!/^[0-9a-f]{40}$/.test(manifest.source.revision)) {
    throw new Error("wmediumd revision must be a full commit");
  }
  if (!Number.isInteger(manifest.radios) || manifest.radios < 2) {
    throw new Error("network environment requires at least two radios");
  }
  for (const version of Object.values(manifest.clients)) {
    if (!/^\d+\.\d+(?:\.\d+)?$/.test(version)) {
      throw new Error("network clients need exact versions");
    }
  }
  for (const value of [
    manifest.base,
    manifest.debianSnapshot,
    "hostapd",
    "wpasupplicant",
    "iw",
    "iproute2",
    "ENVIRONMENT_FINGERPRINT",
  ]) {
    if (!dockerfile.includes(value)) {
      throw new Error(
        `network Dockerfile lacks required pin or tool: ${value}`,
      );
    }
  }
  return manifest;
}

export function environmentFingerprint(manifestSource, dockerfile) {
  return createHash("sha256")
    .update(manifestSource)
    .update("\0")
    .update(dockerfile)
    .digest("hex");
}

function inputs() {
  const manifestSource = readFileSync(MANIFEST_PATH, "utf8");
  const dockerfile = readFileSync(DOCKERFILE_PATH, "utf8");
  return {
    manifest: validateEnvironment(manifestSource, dockerfile),
    manifestSource,
    dockerfile,
  };
}

function dockerRun(manifest, extra) {
  run("docker", [
    "run",
    "--rm",
    "--network=none",
    "--volume",
    `${WORKSPACE}:/source`,
    "--workdir",
    "/source",
    manifest.image,
    ...extra,
  ]);
}

function buildWmediumd(manifest, outputName) {
  dockerRun(manifest, [
    "sh",
    "-ceu",
    `make clean >/dev/null 2>&1 || true; make -j2; cp wmediumd/wmediumd /source/${outputName}`,
  ]);
}

function provision() {
  const { manifest, manifestSource, dockerfile } = inputs();
  if (!existsSync(join(SOURCE, "wmediumd", "wmediumd.c"))) {
    throw new Error(
      "pinned wmediumd source is missing; run make sources-fetch",
    );
  }
  run("docker", [
    "build",
    "--file",
    DOCKERFILE_PATH,
    "--build-arg",
    `DEBIAN_SNAPSHOT=${manifest.debianSnapshot}`,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${environmentFingerprint(manifestSource, dockerfile)}`,
    "--tag",
    manifest.image,
    DIRECTORY,
  ]);
  rmSync(WORKSPACE, { recursive: true, force: true });
  mkdirSync(ARTIFACTS, { recursive: true });
  cpSync(SOURCE, WORKSPACE, { recursive: true });
  buildWmediumd(manifest, "wmediumd.online");
  buildWmediumd(manifest, "wmediumd.offline");
  if (
    sha256(join(WORKSPACE, "wmediumd.online")) !==
    sha256(join(WORKSPACE, "wmediumd.offline"))
  ) {
    throw new Error("repeated wmediumd builds differ");
  }
  cpSync(join(WORKSPACE, "wmediumd.offline"), BINARY);
  run("docker", [
    "run",
    "--rm",
    "--network=none",
    manifest.image,
    "sh",
    "-ceu",
    [
      `test \"$(hostapd -v 2>&1 | head -1)\" = 'hostapd v${manifest.clients.hostapd}'`,
      `test \"$(wpa_supplicant -v 2>&1 | head -1)\" = 'wpa_supplicant v${manifest.clients.wpaSupplicant}'`,
      `test \"$(iw --version)\" = 'iw version ${manifest.clients.iw}'`,
      `ip -V | grep -F 'iproute2-${manifest.clients.iproute2}'`,
    ].join(" && "),
  ]);
  process.stdout.write(`Prepared deterministic wmediumd ${sha256(BINARY)}.\n`);
}

function containerExists(name) {
  return (
    run("docker", ["container", "inspect", name], {
      allowFailure: true,
      capture: true,
      quiet: true,
    }).status === 0
  );
}

function hwsimLoaded() {
  return existsSync("/sys/module/mac80211_hwsim");
}

function cleanup(manifest) {
  if (containerExists(MEDIUM_CONTAINER)) {
    run("docker", ["container", "rm", "--force", MEDIUM_CONTAINER]);
  }
  if (containerExists(manifest.container)) {
    run("docker", ["container", "rm", "--force", manifest.container]);
  }
  if (hwsimLoaded())
    run("sudo", ["-n", "modprobe", "-r", manifest.kernelModule]);
}

function up() {
  const started = performance.now();
  const { manifest } = inputs();
  if (!existsSync(BINARY))
    throw new Error("network environment is not provisioned");
  if (containerExists(manifest.container) || hwsimLoaded()) {
    throw new Error("network environment or mac80211_hwsim is already active");
  }
  try {
    run("docker", [
      "run",
      "--detach",
      "--name",
      manifest.container,
      "--network=none",
      "--privileged",
      "--read-only",
      "--tmpfs",
      "/tmp:rw,nosuid,nodev,size=64m",
      "--tmpfs",
      "/run:rw,nosuid,nodev,size=32m",
      "--volume",
      `${BINARY}:/usr/local/bin/wmediumd:ro`,
      "--volume",
      `${DIRECTORY}:/environment:ro`,
      manifest.image,
      "sleep",
      "infinity",
    ]);
    run("sudo", [
      "-n",
      "modprobe",
      manifest.kernelModule,
      `radios=${manifest.radios}`,
    ]);
    const pid = output("docker", [
      "inspect",
      "--format",
      "{{.State.Pid}}",
      manifest.container,
    ]);
    const phys = output("sh", [
      "-ceu",
      'for p in /sys/class/ieee80211/*; do readlink -f "$p/device/driver/module" | grep -q \'/mac80211_hwsim$\' && basename "$p"; done',
    ]).split("\n");
    if (phys.length !== manifest.radios) {
      throw new Error(
        `created ${phys.length} hwsim PHYs, expected ${manifest.radios}`,
      );
    }
    for (const phy of phys)
      run("sudo", ["-n", "iw", "phy", phy, "set", "netns", pid]);
    const count = Number(
      output("docker", [
        "exec",
        manifest.container,
        "sh",
        "-ceu",
        "iw dev | grep -c '^phy#'",
      ]),
    );
    if (count !== manifest.radios)
      throw new Error(`container sees ${count} radios`);
  } catch (error) {
    cleanup(manifest);
    throw error;
  }
  const elapsed = performance.now() - started;
  if (elapsed > 60_000) throw new Error(`network startup took ${elapsed}ms`);
  process.stdout.write(
    `Network environment ready in ${Math.round(elapsed)}ms.\n`,
  );
}

function down() {
  const started = performance.now();
  const { manifest } = inputs();
  cleanup(manifest);
  const elapsed = performance.now() - started;
  if (elapsed > 60_000) throw new Error(`network teardown took ${elapsed}ms`);
  process.stdout.write(
    `Network environment stopped in ${Math.round(elapsed)}ms.\n`,
  );
}

function selfTest(kind) {
  const { manifest } = inputs();
  if (!containerExists(manifest.container))
    throw new Error("run network-up first");
  if (kind === "netem") {
    run("docker", [
      "exec",
      manifest.container,
      "/environment/netem-self-test.py",
    ]);
    return;
  }
  if (kind === "wifi-direct-client") {
    run("docker", [
      "exec",
      manifest.container,
      "/environment/wifi-direct-self-test.sh",
    ]);
    return;
  }
  if (["lan", "hotspot-client", "hotspot-owner"].includes(kind)) {
    run("docker", [
      "exec",
      manifest.container,
      "/environment/wifi-self-test.sh",
      kind,
    ]);
    return;
  }
  if (kind !== "medium") throw new Error(`unknown network self-test: ${kind}`);
  const script = "/environment/wmediumd-self-test.sh";
  try {
    run("docker", ["exec", manifest.container, script, "setup"]);
    run("docker", ["exec", manifest.container, script, "control"]);
    run("docker", [
      "run",
      "--detach",
      "--name",
      MEDIUM_CONTAINER,
      "--network=host",
      "--cap-add=NET_ADMIN",
      "--read-only",
      "--volume",
      `${BINARY}:/usr/local/bin/wmediumd:ro`,
      "--volume",
      `${join(DIRECTORY, "drop.cfg")}:/drop.cfg:ro`,
      manifest.image,
      "wmediumd",
      "-c",
      "/drop.cfg",
    ]);
    let ready = false;
    for (let attempt = 0; attempt < 50; attempt += 1) {
      const logs = run("docker", ["logs", MEDIUM_CONTAINER], {
        allowFailure: true,
        capture: true,
        quiet: true,
      });
      if (`${logs.stdout}${logs.stderr}`.includes("REGISTER SENT")) {
        ready = true;
        break;
      }
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 20);
    }
    if (!ready)
      throw new Error("wmediumd did not register with mac80211_hwsim");
    run("docker", ["exec", manifest.container, script, "fault"]);
    run("docker", ["container", "rm", "--force", MEDIUM_CONTAINER]);
    run("docker", ["exec", manifest.container, script, "control"]);
    process.stdout.write(
      "wmediumd control-fault-control self-test passed in both directions.\n",
    );
  } finally {
    if (containerExists(MEDIUM_CONTAINER)) {
      run("docker", ["container", "rm", "--force", MEDIUM_CONTAINER]);
    }
    run("docker", ["exec", manifest.container, script, "cleanup"], {
      allowFailure: true,
    });
  }
}

function validate() {
  inputs();
  process.stdout.write("Pinned network environment configuration passed.\n");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  const actions = {
    validate,
    provision,
    up,
    down,
    "self-test": () => selfTest(process.argv[3]),
  };
  if (!actions[command]) throw new Error(`unknown network command: ${command}`);
  actions[command]();
}
