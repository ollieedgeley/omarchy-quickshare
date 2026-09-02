import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer, connect } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const PROXY_CACHE = join(CACHE, "proxies");
const ARTIFACTS = join(PROXY_CACHE, "artifacts");
const STATE = join(PROXY_CACHE, "state.json");
const LOG = join(PROXY_CACHE, "toxiproxy.log");
const SOURCE = join(CACHE, "sources", "trees", "toxiproxy");
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.toolchain");

function run(command, args, options = {}) {
  if (!options.quiet) process.stdout.write(`+ ${command} ${args.join(" ")}\n`);
  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    env: options.env ?? process.env,
    stdio: options.capture ? "pipe" : "inherit",
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const detail = options.capture
      ? `\n${result.stdout ?? ""}${result.stderr ?? ""}`
      : "";
    throw new Error(`${command} exited with ${result.status}${detail}`);
  }
  return result;
}

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function validateEnvironment(manifestSource, dockerfile) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) throw new Error("unsupported proxy schema");
  if (!/^debian@sha256:[0-9a-f]{64}$/.test(manifest.base)) {
    throw new Error("proxy base image must use a SHA-256 digest");
  }
  if (!/^\d{8}T\d{6}Z$/.test(manifest.debianSnapshot)) {
    throw new Error("proxy Debian snapshot must be timestamped");
  }
  if (!/^\d+\.\d+\.\d+$/.test(manifest.go.version)) {
    throw new Error("proxy Go toolchain must use an exact version");
  }
  if (!/^https:\/\/go\.dev\/dl\//.test(manifest.go.url)) {
    throw new Error("proxy Go toolchain must use the official download host");
  }
  if (!/^[0-9a-f]{64}$/.test(manifest.go.sha256)) {
    throw new Error("proxy Go toolchain must include a SHA-256 digest");
  }
  if (!/^[0-9a-f]{40}$/.test(manifest.source.revision)) {
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
  const uid = process.getuid?.() ?? 1000;
  const gid = process.getgid?.() ?? 1000;
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
    `-ldflags=-s -w -X github.com/Shopify/toxiproxy/v2.Version=${manifest.version}`,
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
  await new Promise((accept, reject) =>
    server.close((error) => (error ? reject(error) : accept())),
  );
  return address.port;
}

async function waitForApi(port) {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/version`);
      if (response.ok) return;
    } catch {}
    await new Promise((accept) => setTimeout(accept, 50));
  }
  throw new Error("Toxiproxy API did not become ready");
}

async function up() {
  const started = performance.now();
  const { manifest } = inputs();
  const binary = join(ARTIFACTS, manifest.binary);
  if (!existsSync(binary))
    throw new Error("proxy environment is not provisioned");
  if (existsSync(STATE))
    throw new Error("proxy environment is already running");
  mkdirSync(PROXY_CACHE, { recursive: true });
  const port = await unusedPort();
  const logFd = openSync(LOG, "a");
  const child = spawn(
    binary,
    ["-host", "127.0.0.1", "-port", `${port}`, "-seed", "1"],
    {
      detached: true,
      stdio: ["ignore", logFd, logFd],
    },
  );
  child.unref();
  closeSync(logFd);
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
  if (elapsed > 60_000) throw new Error(`proxy startup took ${elapsed}ms`);
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
  const deadline = Date.now() + 5_000;
  while (liveProcess(state.pid) && Date.now() < deadline) {
    await new Promise((accept) => setTimeout(accept, 25));
  }
  if (liveProcess(state.pid)) process.kill(state.pid, "SIGKILL");
  rmSync(STATE);
  const elapsed = performance.now() - started;
  if (elapsed > 60_000) throw new Error(`proxy teardown took ${elapsed}ms`);
  process.stdout.write(
    `Proxy environment stopped in ${Math.round(elapsed)}ms.\n`,
  );
}

async function request(port, method, path, body) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok)
    throw new Error(`${method} ${path} returned ${response.status}`);
  return response;
}

function transfer(port, payload) {
  return new Promise((accept, reject) => {
    const chunks = [];
    const socket = connect(port, "127.0.0.1", () => socket.write(payload));
    const timer = setTimeout(
      () => socket.destroy(new Error("TCP transfer timed out")),
      5_000,
    );
    socket.on("data", (chunk) => chunks.push(chunk));
    socket.on("error", reject);
    socket.on("close", () => {
      clearTimeout(timer);
      accept(Buffer.concat(chunks));
    });
  });
}

async function selfTest() {
  if (!existsSync(STATE)) throw new Error("run proxy-up before the self-test");
  const { port: apiPort } = JSON.parse(readFileSync(STATE, "utf8"));
  const received = [];
  const upstream = createServer((socket) => {
    socket.once("data", (chunk) => {
      received.push(chunk.length);
      socket.end(chunk);
    });
  });
  await new Promise((accept, reject) => {
    upstream.once("error", reject);
    upstream.listen(0, "127.0.0.1", accept);
  });
  const upstreamPort = upstream.address().port;
  const proxyPort = await unusedPort();
  const payload = Buffer.from("quickshare-proxy-control-payload");
  try {
    await request(apiPort, "POST", "/proxies", {
      name: "quickshare",
      listen: `127.0.0.1:${proxyPort}`,
      upstream: `127.0.0.1:${upstreamPort}`,
      enabled: true,
    });
    const initial = await transfer(proxyPort, payload);
    if (!initial.equals(payload)) {
      throw new Error(
        `initial proxy control returned ${initial.length} bytes, expected ${payload.length}`,
      );
    }
    for (const stream of ["upstream", "downstream"]) {
      await request(apiPort, "POST", "/proxies/quickshare/toxics", {
        name: `cut-${stream}`,
        type: "limit_data",
        stream,
        toxicity: 1,
        attributes: { bytes: 7 },
      });
      const cut = await transfer(proxyPort, payload);
      const observed = stream === "upstream" ? received.at(-1) : cut.length;
      if (observed !== 7) {
        throw new Error(
          `${stream} cutoff exposed ${observed} bytes, expected 7`,
        );
      }
      await request(apiPort, "POST", "/reset");
      if (!(await transfer(proxyPort, payload)).equals(payload)) {
        throw new Error(`${stream} recovery control corrupted data`);
      }
    }
  } finally {
    await new Promise((accept) => upstream.close(accept));
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
  const command = process.argv[2];
  const commands = { validate, provision, up, down, "self-test": selfTest };
  if (!commands[command]) {
    throw new Error(`unknown proxy environment command: ${command}`);
  }
  await commands[command]();
}
