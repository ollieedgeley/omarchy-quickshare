import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { connect, createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const { recordFailureArtifact } = await import(
  new URL("../../../tools/gates/lib/failure-artifact.mjs", import.meta.url)
);
const { run } = await import(
  new URL("../../../tools/gates/lib/process.mjs", import.meta.url)
);

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const PROXY_CACHE = join(CACHE, "proxies");
const ARTIFACTS = join(PROXY_CACHE, "artifacts");
const STATE = join(PROXY_CACHE, "state.json");
const SOURCE = join(CACHE, "sources", "trees", "toxiproxy");
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.toolchain");
const DEFAULT_ID = 1000;
const API_READY_TIMEOUT_MS = 10_000;
const API_POLL_DELAY_MS = 50;
const PROCESS_STOP_TIMEOUT_MS = 5_000;
const PROCESS_POLL_DELAY_MS = 25;
const LIFECYCLE_TIMEOUT_MS = 60_000;
const TRANSFER_TIMEOUT_MS = 5_000;
const CUTOFF_BYTES = 7;
const BASE_IMAGE_PATTERN = /^debian@sha256:[0-9a-f]{64}$/u;
const SNAPSHOT_PATTERN = /^\d{8}T\d{6}Z$/u;
const VERSION_PATTERN = /^\d+\.\d+\.\d+$/u;
const GO_DOWNLOAD_PATTERN = /^https:\/\/go\.dev\/dl\//u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function validateEnvironment(manifestSource, dockerfile) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) {
    throw new Error("unsupported proxy schema");
  }
  if (!BASE_IMAGE_PATTERN.test(manifest.base)) {
    throw new Error("proxy base image must use a SHA-256 digest");
  }
  if (!SNAPSHOT_PATTERN.test(manifest.debianSnapshot)) {
    throw new Error("proxy Debian snapshot must be timestamped");
  }
  if (!VERSION_PATTERN.test(manifest.go.version)) {
    throw new Error("proxy Go toolchain must use an exact version");
  }
  if (!GO_DOWNLOAD_PATTERN.test(manifest.go.url)) {
    throw new Error("proxy Go toolchain must use the official download host");
  }
  if (!SHA256_PATTERN.test(manifest.go.sha256)) {
    throw new Error("proxy Go toolchain must include a SHA-256 digest");
  }
  if (!REVISION_PATTERN.test(manifest.source.revision)) {
    throw new Error("Toxiproxy revision must be a full commit");
  }
  for (const value of [
    manifest.base,
    manifest.debianSnapshot,
    manifest.go.url,
    manifest.go.sha256,
    "ENVIRONMENT_FINGERPRINT",
  ]) {
    if (!dockerfile.includes(value)) {
      throw new Error(`Dockerfile lacks manifest value or pin: ${value}`);
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

function dockerBuild(manifest, fingerprint) {
  run("docker", [
    "build",
    "--file",
    DOCKERFILE_PATH,
    "--build-arg",
    `DEBIAN_SNAPSHOT=${manifest.debianSnapshot}`,
    "--build-arg",
    `GO_URL=${manifest.go.url}`,
    "--build-arg",
    `GO_SHA256=${manifest.go.sha256}`,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${fingerprint}`,
    "--tag",
    manifest.image,
    DIRECTORY,
  ]);
}

function buildBinary(manifest, output, network) {
  const uid = process.getuid?.() ?? DEFAULT_ID;
  const gid = process.getgid?.() ?? DEFAULT_ID;
  const goCache = join(PROXY_CACHE, "go");
  mkdirSync(goCache, { recursive: true });
  run("docker", [
    "run",
    "--rm",
    `--network=${network}`,
    "--user",
    `${uid}:${gid}`,
    "--env",
    "HOME=/go/home",
    "--env",
    "GOCACHE=/go/build",
    "--env",
    "GOMODCACHE=/go/mod",
    "--env",
    "CGO_ENABLED=0",
    "--volume",
    `${SOURCE}:/source:ro`,
    "--volume",
    `${ARTIFACTS}:/artifacts`,
    "--volume",
    `${goCache}:/go`,
    "--workdir",
    "/source",
    manifest.image,
    "go",
    "build",
    "-mod=readonly",
    "-trimpath",
    "-ldflags=-s -w -X " +
      `github.com/Shopify/toxiproxy/v2.Version=${manifest.version}`,
    "-o",
    `/artifacts/${output}`,
    "./cmd/server",
  ]);
}

function provision() {
  const { manifest, manifestSource, dockerfile } = inputs();
  if (!existsSync(join(SOURCE, "go.sum"))) {
    throw new Error(
      "pinned Toxiproxy source is missing; run make sources-fetch",
    );
  }
  mkdirSync(ARTIFACTS, { recursive: true });
  dockerBuild(manifest, environmentFingerprint(manifestSource, dockerfile));
  buildBinary(manifest, manifest.binary, "bridge");
  buildBinary(manifest, `${manifest.binary}.offline`, "none");
  const binary = join(ARTIFACTS, manifest.binary);
  const offline = `${binary}.offline`;
  if (sha256(binary) !== sha256(offline)) {
    throw new Error("online and offline Toxiproxy builds differ");
  }
  rmSync(offline);
  run(binary, ["-version"]);
  process.stdout.write(
    `Prepared deterministic proxy binary ${sha256(binary)}.\n`,
  );
}

async function unusedPort() {
  const server = createServer();
  await new Promise((accept, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", accept);
  });
  const address = server.address();
  await new Promise((accept, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      accept();
    });
  });
  return address.port;
}

function delay(milliseconds) {
  return new Promise((accept) => {
    setTimeout(accept, milliseconds);
  });
}

async function apiIsReady(port) {
  try {
    const response = await fetch(`http://127.0.0.1:${port}/version`);
    return response.ok;
  } catch {
    return false;
  }
}

async function pollApi(port, deadline) {
  if (await apiIsReady(port)) {
    return;
  }
  if (Date.now() >= deadline) {
    throw new Error("Toxiproxy API did not become ready");
  }
  await delay(API_POLL_DELAY_MS);
  await pollApi(port, deadline);
}

async function waitForApi(port) {
  await pollApi(port, Date.now() + API_READY_TIMEOUT_MS);
}

async function up() {
  const started = performance.now();
  const { manifest } = inputs();
  const binary = join(ARTIFACTS, manifest.binary);
  if (!existsSync(binary)) {
    throw new Error("proxy environment is not provisioned");
  }
  if (existsSync(STATE)) {
    throw new Error("proxy environment is already running");
  }
  mkdirSync(PROXY_CACHE, { recursive: true });
  const port = await unusedPort();
  const child = spawn(
    binary,
    ["-host", "127.0.0.1", "-port", `${port}`, "-seed", "1"],
    {
      detached: true,
      stdio: "ignore",
    },
  );
  child.unref();
  try {
    await waitForApi(port);
    writeFileSync(
      STATE,
      `${JSON.stringify({ pid: child.pid, port, binary })}\n`,
    );
  } catch (error) {
    process.kill(child.pid, "SIGTERM");
    throw error;
  }
  const elapsed = performance.now() - started;
  if (elapsed > LIFECYCLE_TIMEOUT_MS) {
    throw new Error(`proxy startup took ${elapsed}ms`);
  }
  process.stdout.write(
    `Proxy environment ready in ${Math.round(elapsed)}ms.\n`,
  );
}

function liveProcess(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function waitForProcessStop(pid, deadline) {
  if (!liveProcess(pid) || Date.now() >= deadline) {
    return;
  }
  await delay(PROCESS_POLL_DELAY_MS);
  await waitForProcessStop(pid, deadline);
}

async function down() {
  const started = performance.now();
  if (!existsSync(STATE)) {
    process.stdout.write("Proxy environment is already stopped.\n");
    return;
  }
  const state = JSON.parse(readFileSync(STATE, "utf8"));
  const command = readFileSync(`/proc/${state.pid}/cmdline`, "utf8");
  if (!command.includes(state.binary)) {
    throw new Error(`refusing to stop unexpected process ${state.pid}`);
  }
  process.kill(state.pid, "SIGTERM");
  await waitForProcessStop(state.pid, Date.now() + PROCESS_STOP_TIMEOUT_MS);
  if (liveProcess(state.pid)) {
    process.kill(state.pid, "SIGKILL");
  }
  rmSync(STATE);
  const elapsed = performance.now() - started;
  if (elapsed > LIFECYCLE_TIMEOUT_MS) {
    throw new Error(`proxy teardown took ${elapsed}ms`);
  }
  process.stdout.write(
    `Proxy environment stopped in ${Math.round(elapsed)}ms.\n`,
  );
}

async function request({ body, method, path, port }) {
  const options = {
    headers: { "content-type": "application/json" },
    method,
  };
  if (body) {
    options.body = JSON.stringify(body);
  }
  const response = await fetch(`http://127.0.0.1:${port}${path}`, options);
  if (!response.ok) {
    throw new Error(`${method} ${path} returned ${response.status}`);
  }
  return response;
}

function transfer(port, payload) {
  return new Promise((accept, reject) => {
    const chunks = [];
    const socket = connect(port, "127.0.0.1", () => {
      socket.write(payload);
    });
    const timer = setTimeout(
      () => socket.destroy(new Error("TCP transfer timed out")),
      TRANSFER_TIMEOUT_MS,
    );
    socket.on("data", (chunk) => {
      chunks.push(chunk);
    });
    socket.on("error", reject);
    socket.on("close", () => {
      clearTimeout(timer);
      accept(Buffer.concat(chunks));
    });
  });
}

export function trackUpstreamSocket(socket, received, failures) {
  socket.on("error", (error) => {
    if (error.code !== "ECONNRESET") {
      failures.push(error.code ?? "UNKNOWN");
    }
  });
  socket.once("data", (chunk) => {
    received.push(chunk.length);
    socket.end(chunk);
  });
}

async function startUpstream(received, failures) {
  const upstream = createServer((socket) => {
    trackUpstreamSocket(socket, received, failures);
  });
  await new Promise((accept, reject) => {
    upstream.once("error", reject);
    upstream.listen(0, "127.0.0.1", accept);
  });
  return upstream;
}

async function createProxy(apiPort, proxyPort, upstreamPort) {
  await request({
    body: {
      enabled: true,
      listen: `127.0.0.1:${proxyPort}`,
      name: "quickshare",
      upstream: `127.0.0.1:${upstreamPort}`,
    },
    method: "POST",
    path: "/proxies",
    port: apiPort,
  });
}

function assertUpstreamHealthy(context) {
  if (context.upstreamFailures.length > 0) {
    throw new Error("proxy upstream echo encountered a socket failure");
  }
}

async function testStream(context, stream) {
  await request({
    body: {
      attributes: { bytes: CUTOFF_BYTES },
      name: `cut-${stream}`,
      stream,
      toxicity: 1,
      type: "limit_data",
    },
    method: "POST",
    path: "/proxies/quickshare/toxics",
    port: context.apiPort,
  });
  const cut = await transfer(context.proxyPort, context.payload);
  assertUpstreamHealthy(context);
  let observed = cut.length;
  if (stream === "upstream") {
    observed = context.received.at(-1);
  }
  if (observed !== CUTOFF_BYTES) {
    throw new Error(
      `${stream} cutoff exposed ${observed} bytes, ` +
        `expected ${CUTOFF_BYTES}`,
    );
  }
  await request({ method: "POST", path: "/reset", port: context.apiPort });
  if (
    !(await transfer(context.proxyPort, context.payload)).equals(
      context.payload,
    )
  ) {
    throw new Error(`${stream} recovery control corrupted data`);
  }
  assertUpstreamHealthy(context);
}

async function runProxyProof(context) {
  await createProxy(
    context.apiPort,
    context.proxyPort,
    context.upstream.address().port,
  );
  const initial = await transfer(context.proxyPort, context.payload);
  assertUpstreamHealthy(context);
  if (!initial.equals(context.payload)) {
    throw new Error(
      `initial proxy control returned ${initial.length} bytes, ` +
        `expected ${context.payload.length}`,
    );
  }
  await testStream(context, "upstream");
  await testStream(context, "downstream");
}

async function selfTest() {
  try {
    if (!existsSync(STATE)) {
      throw new Error("run proxy-up before the self-test");
    }
    const { port: apiPort } = JSON.parse(readFileSync(STATE, "utf8"));
    const received = [];
    const upstreamFailures = [];
    const upstream = await startUpstream(received, upstreamFailures);
    const context = {
      apiPort,
      payload: Buffer.from("quickshare-proxy-control-payload"),
      proxyPort: await unusedPort(),
      received,
      upstream,
      upstreamFailures,
    };
    try {
      await runProxyProof(context);
    } finally {
      await new Promise((accept) => {
        upstream.close(accept);
      });
    }
  } catch (error) {
    recordFailureArtifact(join(ROOT, "reports", "failures"), {
      events: [{ event: "toxiproxy", status: "failed" }],
      gate: "toxiproxy",
      outcome: { kind: "failed" },
      stage: "proxy-proof",
    });
    throw error;
  }
  process.stdout.write(
    "Toxiproxy control-fault-control self-test passed in both directions.\n",
  );
}

function validate() {
  inputs();
  process.stdout.write("Pinned proxy environment configuration passed.\n");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , command] = process.argv;
  const commands = {
    down,
    provision,
    "self-test": selfTest,
    up,
    validate,
  };
  if (!commands[command]) {
    throw new Error(`unknown proxy environment command: ${command}`);
  }
  await commands[command]();
}
