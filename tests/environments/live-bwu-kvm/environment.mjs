import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { createConnection as connect } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";

import { runPeerPair } from "./peer-pair.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const RADIO_CACHE = join(CACHE, "bluetooth-radio");
const BTVIRT = join(RADIO_CACHE, "artifacts", "btvirt");
const RUNTIME = join(CACHE, "live-bwu-kvm", "runtime");
const MANIFEST = join(DIRECTORY, "environment.json");
const DOCKERFILE = join(DIRECTORY, "Dockerfile");
const NEARBY_STATE = join(CACHE, "nearby-linux", "state.json");
const ADDRESS_PATTERN = /ADDRESS (?<address>[0-9A-F:]{17})/u;
const IMAGE_PATTERN = /^omarchy-quickshare\/.+:[\w.-]+$/u;
const CONTROL_TIMEOUT_MS = 20_000;
const POLL_INTERVAL_MS = 100;
const READY = "READY\n";
const STATUS_SUCCESS = "STATUS 0";
const PEERS = ["a", "b"];
function controlPath(peer) {
  return join(RUNTIME, `${peer}.control.sock`);
}
function sidecarName(config) {
  return `${config.container}-btvirt`;
}
function stdio(options) {
  if (options.capture) {
    return "pipe";
  }
  return "inherit";
}
function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    stdio: stdio(options),
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0 && !options.allowFailure) {
    throw new Error(`${command} failed with ${result.status}`);
  }
  return result.stdout ?? "";
}
function manifest() {
  const parsed = JSON.parse(readFileSync(MANIFEST, "utf8"));
  if (
    parsed.schema !== 1 ||
    ![parsed.image, parsed.nearbyImage, parsed.radioImage].every((image) =>
      IMAGE_PATTERN.test(image),
    )
  ) {
    throw new Error("invalid live-BWU KVM environment manifest");
  }
  return parsed;
}
function imageId(image) {
  return run("docker", ["image", "inspect", "--format", "{{.Id}}", image], {
    capture: true,
  }).trim();
}
function fingerprint(config) {
  return createHash("sha256")
    .update(readFileSync(MANIFEST, "utf8"))
    .update("\0")
    .update(readFileSync(DOCKERFILE, "utf8"))
    .update("\0")
    .update(imageId(config.nearbyImage))
    .update("\0")
    .update(imageId(config.radioImage))
    .digest("hex");
}
function imageFingerprint(image) {
  return run(
    "docker",
    [
      "image",
      "inspect",
      "--format",
      '{{index .Config.Labels "org.omarchy-quickshare.fingerprint"}}',
      image,
    ],
    { allowFailure: true, capture: true },
  ).trim();
}
function isRunning(container) {
  return (
    run("docker", ["container", "inspect", container], {
      allowFailure: true,
      capture: true,
    }).trim() !== "[]"
  );
}
function prepared(config) {
  if (!existsSync(BTVIRT)) {
    throw new Error(
      "Bluetooth radio artifact missing; run bluetooth-radio-provision",
    );
  }
  if (
    !run("docker", ["image", "inspect", config.radioImage], {
      allowFailure: true,
      capture: true,
    })
  ) {
    throw new Error(
      "Bluetooth radio image missing; run bluetooth-radio-provision",
    );
  }
  if (!existsSync(NEARBY_STATE)) {
    throw new Error(
      "Nearby Linux image is missing; run nearby-linux-provision",
    );
  }
  const nearby = JSON.parse(readFileSync(NEARBY_STATE, "utf8"));
  if (imageFingerprint(config.nearbyImage) !== nearby.fingerprint) {
    throw new Error("Nearby Linux image is stale; run nearby-linux-provision");
  }
  if (imageFingerprint(config.image) !== fingerprint(config)) {
    throw new Error("live-BWU peer image is stale; run live-bwu-kvm-provision");
  }
}
function provision(config) {
  if (!existsSync(BTVIRT)) {
    throw new Error(
      "Bluetooth radio artifact missing; run bluetooth-radio-provision",
    );
  }
  if (!existsSync(NEARBY_STATE)) {
    throw new Error(
      "Nearby Linux image is missing; run nearby-linux-provision",
    );
  }
  run("docker", ["image", "inspect", config.radioImage], { capture: true });
  const nearby = JSON.parse(readFileSync(NEARBY_STATE, "utf8"));
  if (imageFingerprint(config.nearbyImage) !== nearby.fingerprint) {
    throw new Error("Nearby Linux image is stale; run nearby-linux-provision");
  }
  run("docker", [
    "build",
    "--provenance=false",
    "--build-arg",
    `NEARBY_IMAGE=${config.nearbyImage}`,
    "--build-arg",
    `RADIO_IMAGE=${config.radioImage}`,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${fingerprint(config)}`,
    "--tag",
    config.image,
    "--file",
    DOCKERFILE,
    DIRECTORY,
  ]);
  prepared(config);
  process.stdout.write("Prepared sealed live-BWU KVM peer image.\n");
}
function cleanup(config) {
  const sidecar = sidecarName(config);
  if (isRunning(sidecar)) {
    run("docker", ["container", "rm", "--force", sidecar]);
  }
  if (isRunning(config.container)) {
    run("docker", ["container", "rm", "--force", config.container]);
  }
  rmSync(RUNTIME, { recursive: true, force: true });
}
function waitForControl(path) {
  const deadline = Date.now() + CONTROL_TIMEOUT_MS;
  return new Promise((resolveReady, rejectReady) => {
    const check = () => {
      if (existsSync(path)) {
        resolveReady();
        return;
      }
      if (Date.now() >= deadline) {
        rejectReady(new Error(`control socket not ready: ${path}`));
        return;
      }
      setTimeout(check, POLL_INTERVAL_MS);
    };
    check();
  });
}
export async function guestTransaction({ peer, command = "", expected }) {
  const path = controlPath(peer);
  await waitForControl(path);
  return new Promise((resolveResult, rejectResult) => {
    const state = { client: null, completed: false, output: "", sent: false };
    const deadline = Date.now() + CONTROL_TIMEOUT_MS;
    const timeout = setTimeout(() => {
      state.client?.destroy();
      rejectResult(new Error(`guest command timed out: ${state.output}`));
    }, CONTROL_TIMEOUT_MS);
    const open = () => {
      state.client = connect(path);
      state.client.setEncoding("utf8");
      state.client.on("data", (chunk) => {
        state.output += chunk;
        if (command && state.output.includes(READY) && !state.sent) {
          state.sent = true;
          state.client.write(`${command}\n`);
        }
        if (state.output.includes("STATUS 1")) {
          state.completed = true;
          clearTimeout(timeout);
          state.client.end();
          rejectResult(new Error(`guest command failed: ${state.output}`));
          return;
        }
        if (state.output.includes(expected)) {
          state.completed = true;
          clearTimeout(timeout);
          state.client.once("close", () => resolveResult(state.output));
          state.client.end();
        }
      });
      state.client.once("error", (error) => {
        state.client.destroy();
        if (state.completed) {
          return;
        }
        if (Date.now() < deadline) {
          setTimeout(open, POLL_INTERVAL_MS);
          return;
        }
        clearTimeout(timeout);
        rejectResult(error);
      });
    };
    open();
  });
}
function selectedPeers() {
  const peers = process.env.OQS_PEERS ?? "two";
  if (!new Set(["one", "two"]).has(peers)) {
    throw new Error("OQS_PEERS must be one or two");
  }
  return peers;
}
function startSidecar(config) {
  run("docker", [
    "run",
    "--detach",
    "--name",
    sidecarName(config),
    "--network=none",
    "--user",
    "1000:1000",
    "--read-only",
    "--security-opt",
    "seccomp=unconfined",
    "--tmpfs",
    "/tmp:rw,nosuid,nodev,size=16m",
    "--volume",
    `${BTVIRT}:/usr/local/bin/btvirt:ro`,
    "--volume",
    `${RUNTIME}:/runtime`,
    config.radioImage,
    "btvirt",
    "-d",
    "-s/runtime",
  ]);
}
function startRunner(config, peers) {
  run("docker", [
    "run",
    "--detach",
    "--name",
    config.container,
    "--network=none",
    "--device",
    "/dev/kvm",
    "--user",
    "1000:1000",
    "--env",
    `OQS_PEERS=${peers}`,
    "--volume",
    `${DIRECTORY}:/environment:ro`,
    "--volume",
    `${BTVIRT}:/usr/local/bin/btvirt:ro`,
    "--volume",
    `${RUNTIME}:/runtime`,
    config.image,
    "/environment/supervisor.sh",
  ]);
}
function failureEvidence(config) {
  const log = run("docker", ["logs", config.container, "--tail", "80"], {
    allowFailure: true,
    capture: true,
  });
  const consoles = PEERS.map((peer) => {
    const path = join(RUNTIME, `${peer}.console.log`);
    if (!existsSync(path)) {
      return "";
    }
    return `${peer}: ${readFileSync(path, "utf8")}`;
  }).join("\n");
  return `${log}\n${consoles}`;
}
async function up() {
  const config = manifest();
  const peers = selectedPeers();
  prepared(config);
  if (isRunning(config.container)) {
    throw new Error("live-BWU KVM environment is already running");
  }
  rmSync(RUNTIME, { recursive: true, force: true });
  mkdirSync(RUNTIME, { recursive: true });
  startSidecar(config);
  startRunner(config, peers);
  try {
    await guestTransaction({ peer: "a", expected: READY });
    if (peers === "two") {
      await guestTransaction({ peer: "b", expected: READY });
    }
  } catch (error) {
    const evidence = failureEvidence(config);
    cleanup(config);
    throw new Error(`${error.message}\n${evidence}`, { cause: error });
  }
  process.stdout.write("Started two isolated KVM Bluetooth peers.\n");
}
async function bringUp(peer) {
  await guestTransaction({
    peer,
    command: "BRING_UP",
    expected: STATUS_SUCCESS,
  });
}
function identity(peer) {
  return guestTransaction({
    peer,
    command: "IDENTITY",
    expected: STATUS_SUCCESS,
  });
}
function controllerAddress(identityA, identityB) {
  const address = identityA.match(ADDRESS_PATTERN)?.groups?.address;
  if (
    !address ||
    !identityA.includes("HCI_COUNT 1") ||
    !identityB.includes("HCI_COUNT 1")
  ) {
    throw new Error("guests did not each exclusively own one controller");
  }
  return address;
}
async function testLan() {
  process.stdout.write("Checking guest LAN reference bytes.\n");
  await guestTransaction({
    peer: "a",
    command: "LAN_LISTEN",
    expected: STATUS_SUCCESS,
  });
  await guestTransaction({
    peer: "b",
    command: "LAN_SEND 192.0.2.1",
    expected: "LAN_BYTES_OK",
  });
}
async function testClassic(address) {
  process.stdout.write("Checking guest Classic reference bytes.\n");
  await guestTransaction({
    peer: "a",
    command: "CLASSIC_LISTEN",
    expected: STATUS_SUCCESS,
  });
  await guestTransaction({
    peer: "b",
    command: `CLASSIC_SEND ${address}`,
    expected: "CLASSIC_BYTES_OK",
  });
  const results = await guestTransaction({
    peer: "a",
    command: "RESULTS",
    expected: STATUS_SUCCESS,
  });
  if (!results.includes("RESULTS classic,lan")) {
    throw new Error("reference servers did not receive both byte streams");
  }
}
async function selfTest() {
  const config = manifest();
  if (!isRunning(config.container)) {
    throw new Error("live-BWU KVM environment is not running");
  }
  process.stdout.write("Checking guest controller ownership.\n");
  await bringUp("a");
  await bringUp("b");
  const identityA = await identity("a");
  const identityB = await identity("b");
  const address = controllerAddress(identityA, identityB);
  await testLan();
  await testClassic(address);
  process.stdout.write(
    "Verified isolated controller ownership, LAN bytes, and Classic bytes.\n",
  );
}
async function peerPair() {
  const config = manifest();
  if (!isRunning(config.container)) {
    throw new Error("live-BWU KVM environment is not running");
  }
  await runPeerPair(guestTransaction);
  process.stdout.write(
    "Verified BLE to Bluetooth Classic upgrade and payload SHA-256.\n",
  );
}
function down() {
  cleanup(manifest());
  process.stdout.write("Stopped live-BWU KVM environment.\n");
}
function main() {
  const [, , action] = process.argv;
  if (action === "provision") {
    return provision(manifest());
  }
  if (action === "up") {
    return up();
  }
  if (action === "self-test") {
    return selfTest();
  }
  if (action === "peer-pair") {
    return peerPair();
  }
  if (action === "down") {
    return down();
  }
  throw new Error(
    "usage: environment.mjs {provision|up|self-test|peer-pair|down}",
  );
}
Promise.resolve(main()).catch((error) => {
  process.stderr.write(`${error.message}\n`);
  process.exitCode = 1;
});
