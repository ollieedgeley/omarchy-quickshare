import { createHash } from "node:crypto";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../tools/gates/lib/process.mjs";
import { parseSources } from "../../../tools/gates/sources.mjs";

const { recordFailureArtifact } = await import(
  new URL("../../../tools/gates/lib/failure-artifact.mjs", import.meta.url)
);
import { createComposeRunner } from "./compose-runner.mjs";
import { runConnectionsSelfTest } from "./connections-self-test.mjs";
import {
  contextFingerprint,
  prepareContext,
  treeFingerprint,
} from "./context.mjs";
import { runSharingActionsSelfTest } from "./sharing/actions.mjs";
import { runSharingSelfTest } from "./sharing/transfer.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE_ROOT =
  process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const ENVIRONMENT_ROOT = join(CACHE_ROOT, "nearby-linux");
const SOURCE_ROOT = join(CACHE_ROOT, "sources", "trees");
const STATE_PATH = join(ENVIRONMENT_ROOT, "state.json");
const CASE_ROOT = join(ENVIRONMENT_ROOT, "cases");
const FAILURE_REPORTS = join(ROOT, "reports", "failures");
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile");
const COMPOSE_PATH = join(DIRECTORY, "compose.yaml");
const CLI_ACTIONS_PATCH = join(DIRECTORY, "cli-actions.patch");
const CONTEXT_PATH = join(DIRECTORY, "context.mjs");
const OVERLAY_ROOT = join(ROOT, "tests", "environments", "oracle", "overlays");
const SOURCE_MANIFEST_PATH = join(ROOT, "upstream", "sources.toml");
const BAZEL_BINARY = join(
  CACHE_ROOT,
  "nearby-linux",
  "bazel-9.0.1-linux-x86_64",
);
const LLVM_KEY = join(CACHE_ROOT, "nearby-linux", "llvm-apt-signing-key.asc");
const LIFECYCLE_GOAL_MS = 30_000;
const SELF_TEST_LIMIT_MS = 60_000;
const CASE_MODE = 0o777;
const BASE_IMAGE_PATTERN = /^ubuntu@sha256:[0-9a-f]{64}$/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const VERSION_PATTERN = /^\d+\.\d+\.\d+$/u;
const PACKAGE_PATTERN = /^[a-z0-9.+-]+=\S+$/u;
const OPEN_BRACE = "{";
const SOURCE_IDS = [
  "nearby-linux",
  "gloop",
  "google-ukey2",
  "smhasher",
  "nlohmann-json",
  "nisaba",
  "protobuf-matchers",
  "sdbus-cpp",
];
const REQUIRED_DOCKERFILE_VALUES = [
  "COPY nearby/ ./",
  "COPY overrides/ /workspace/overrides/",
  "COPY cache/repository/ /var/cache/bazel-repository/",
  "--lockfile_mode=error",
  "--override_repository=sdbus_cpp=/workspace/overrides/sdbus-cpp",
  "//connections/file_share:file_share",
  "//sharing/linux:nearby_sharing_cli",
  `FROM $${OPEN_BRACE}UBUNTU_IMAGE} AS runtime`,
];
const REQUIRED_COMPOSE_VALUES = [
  "peer-a:",
  "peer-b:",
  "NEARBY_LINUX_BUILD_CONTEXT",
  "NEARBY_LINUX_IMAGE",
  "healthcheck:",
  "internal: true",
];

function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

function assertSortedUnique(values, description) {
  if (
    !Array.isArray(values) ||
    values.length === 0 ||
    new Set(values).size !== values.length ||
    [...values].sort().join("\0") !== values.join("\0")
  ) {
    throw new Error(`${description} must be a non-empty sorted unique list`);
  }
}

function validatePackages(packages) {
  for (const [name, values] of Object.entries(packages)) {
    assertSortedUnique(values, `Nearby Linux ${name} packages`);
    if (values.some((value) => !PACKAGE_PATTERN.test(value))) {
      throw new Error(`Nearby Linux ${name} packages must be exact pins`);
    }
  }
}

function validateManifest(manifest) {
  if (
    manifest.schema !== 1 ||
    !BASE_IMAGE_PATTERN.test(manifest.ubuntu?.base)
  ) {
    throw new Error("Nearby Linux base image must use a SHA-256 digest");
  }
  if (!manifest.ubuntu.snapshotUrl.startsWith("https://snapshot.ubuntu.com/")) {
    throw new Error("Nearby Linux Ubuntu snapshot must be immutable");
  }
  if (!VERSION_PATTERN.test(manifest.bazel?.version)) {
    throw new Error("Nearby Linux Bazel version must be exact");
  }
  if (!SHA256_PATTERN.test(manifest.bazel.sha256)) {
    throw new Error("Nearby Linux Bazel binary requires a SHA-256");
  }
  if (!SHA256_PATTERN.test(manifest.llvm?.keySha256)) {
    throw new Error("Nearby Linux LLVM signing key requires a SHA-256");
  }
  if (!Number.isInteger(manifest.llvm.version) || manifest.llvm.version < 1) {
    throw new Error("Nearby Linux LLVM version must be a positive integer");
  }
  assertSortedUnique(manifest.sources, "Nearby Linux sources");
  if (manifest.sources.join("\0") !== [...SOURCE_IDS].sort().join("\0")) {
    throw new Error("Nearby Linux source inputs are incomplete");
  }
  validatePackages(manifest.packages);
}

function validateText({ compose, dockerfile, manifest, patch }) {
  for (const value of REQUIRED_DOCKERFILE_VALUES) {
    if (!dockerfile.includes(value)) {
      throw new Error(`Nearby Linux Dockerfile lacks required value: ${value}`);
    }
  }
  for (const value of REQUIRED_COMPOSE_VALUES) {
    if (!compose.includes(value)) {
      throw new Error(
        `Nearby Linux Compose file lacks required value: ${value}`,
      );
    }
  }
  if (!dockerfile.includes(manifest.ubuntu.base)) {
    throw new Error("Nearby Linux Dockerfile lacks the pinned Ubuntu image");
  }
  if (!patch.includes("QS_EVENT")) {
    throw new Error("Nearby Linux CLI patch lacks machine-readable evidence");
  }
  run("git", ["apply", "--numstat", "-"], {
    capture: true,
    input: patch,
    quiet: true,
  });
}

export function validateEnvironment(configuration) {
  const manifest = JSON.parse(configuration.manifestSource);
  validateManifest(manifest);
  validateText({ ...configuration, manifest });
  return manifest;
}

export function environmentFingerprint(configuration) {
  return contextFingerprint(configuration);
}

function inputs() {
  const assets = treeFingerprint(join(DIRECTORY, "assets"));
  const compose = readFileSync(COMPOSE_PATH, "utf8");
  const contextSource = readFileSync(CONTEXT_PATH, "utf8");
  const patch = readFileSync(CLI_ACTIONS_PATCH, "utf8");
  const dockerfile = readFileSync(DOCKERFILE_PATH, "utf8");
  const connectionsPeer = treeFingerprint(
    join(ROOT, "tools", "oracle", "connections-peer"),
  );
  const fixtureGenerator = treeFingerprint(
    join(ROOT, "tools", "oracle", "sharing-fixtures"),
  );
  const manifestSource = readFileSync(MANIFEST_PATH, "utf8");
  const overlays = treeFingerprint(OVERLAY_ROOT);
  const sources = readFileSync(SOURCE_MANIFEST_PATH, "utf8");
  const input = {
    assets,
    compose,
    connectionsPeer,
    contextSource,
    dockerfile,
    fixtureGenerator,
    manifestSource,
    overlays,
    patch,
    sources,
  };
  const manifest = validateEnvironment(input);
  return {
    compose,
    dockerfile,
    fingerprint: environmentFingerprint(input),
    manifest,
    manifestSource,
    patch,
  };
}

function dockerCompose(args, environment) {
  return run(
    process.env.DOCKER ?? "docker",
    ["compose", "--file", COMPOSE_PATH, ...args],
    {
      env: environment,
    },
  );
}

function state() {
  if (!existsSync(STATE_PATH)) {
    throw new Error(
      "Nearby Linux is not provisioned; run nearby-linux-provision",
    );
  }
  return JSON.parse(readFileSync(STATE_PATH, "utf8"));
}

function sourceRecords(manifest) {
  const records = parseSources(readFileSync(SOURCE_MANIFEST_PATH, "utf8"));
  const selected = records.filter(({ id }) => manifest.sources.includes(id));
  if (selected.length !== manifest.sources.length) {
    throw new Error(
      "Nearby Linux source manifest does not contain every input",
    );
  }
  return selected;
}

function assertPrepared(manifest, fingerprint) {
  const label = output(process.env.DOCKER ?? "docker", [
    "image",
    "inspect",
    "--format",
    '{{index .Config.Labels "org.omarchy-quickshare.fingerprint"}}',
    manifest.image,
  ]);
  if (label !== fingerprint) {
    throw new Error("Nearby Linux image is stale; run nearby-linux-provision");
  }
}

function buildEnvironment(manifest, fingerprint, context) {
  return {
    ...process.env,
    BUILDER_BASE_PACKAGES: manifest.packages.builder.join(" "),
    BUILDER_LLVM_PACKAGES: manifest.packages.llvm.join(" "),
    ENVIRONMENT_FINGERPRINT: fingerprint,
    LLVM_APT_SOURCE: manifest.llvm.aptSource,
    LLVM_SIGNING_CERT_SHA256: manifest.llvm.keySha256,
    NEARBY_LINUX_BUILD_CONTEXT: context,
    NEARBY_LINUX_DOCKERFILE: "Dockerfile",
    NEARBY_LINUX_IMAGE: manifest.image,
    QUICKSHARE_CASE_A_DIR: join(CASE_ROOT, "prepared-a"),
    QUICKSHARE_CASE_B_DIR: join(CASE_ROOT, "prepared-b"),
    RUNTIME_PACKAGES: manifest.packages.runtime.join(" "),
    UBUNTU_APT_SNAPSHOT_URL: manifest.ubuntu.snapshotUrl,
  };
}

function validateSourceTrees(records) {
  for (const record of records) {
    if (!existsSync(join(SOURCE_ROOT, record.id))) {
      throw new Error(
        `missing pinned source ${record.id}; run make sources-fetch`,
      );
    }
  }
}

function downloadVerified(url, expected, destination) {
  if (
    existsSync(destination) &&
    digest(readFileSync(destination)) === expected
  ) {
    return;
  }
  const temporary = `${destination}.temporary`;
  rmSync(temporary, { force: true });
  mkdirSync(dirname(destination), { recursive: true });
  run("curl", ["--fail", "--location", "--output", temporary, url]);
  if (digest(readFileSync(temporary)) !== expected) {
    rmSync(temporary, { force: true });
    throw new Error(`Nearby Linux download SHA-256 mismatch: ${url}`);
  }
  renameSync(temporary, destination);
}

function prepareToolInputs(manifest) {
  downloadVerified(manifest.bazel.url, manifest.bazel.sha256, BAZEL_BINARY);
  downloadVerified(manifest.llvm.keyUrl, manifest.llvm.keySha256, LLVM_KEY);
}

function provision() {
  const { fingerprint, manifest } = inputs();
  const records = sourceRecords(manifest);
  validateSourceTrees(records);
  prepareToolInputs(manifest);
  const context = prepareContext({
    bazel: BAZEL_BINARY,
    cacheRoot: CACHE_ROOT,
    connectionsPeer: join(ROOT, "tools", "oracle", "connections-peer"),
    environment: DIRECTORY,
    fingerprint,
    fixtureGenerator: join(ROOT, "tools", "oracle", "sharing-fixtures"),
    llvmKey: LLVM_KEY,
    overlayRoot: OVERLAY_ROOT,
    sourceRoot: SOURCE_ROOT,
  });
  const environment = buildEnvironment(manifest, fingerprint, context);
  dockerCompose(["build", "--pull", "--provenance=false"], environment);
  assertPrepared(manifest, fingerprint);
  mkdirSync(ENVIRONMENT_ROOT, { recursive: true });
  const imageId = output(process.env.DOCKER ?? "docker", [
    "image",
    "inspect",
    "--format",
    "{{.Id}}",
    manifest.image,
  ]);
  writeFileSync(
    STATE_PATH,
    `${JSON.stringify({ context, fingerprint, imageId }, null, 2)}\n`,
  );
  process.stdout.write(`Prepared Nearby Linux image ${imageId}.\n`);
}

function caseDirectories() {
  const root = join(CASE_ROOT, `${Date.now()}-${process.pid}`);
  const paths = {
    peerA: join(root, "a"),
    peerB: join(root, "b"),
    root,
  };
  for (const path of [paths.peerA, paths.peerB]) {
    mkdirSync(path, { recursive: true });
    chmodSync(path, CASE_MODE);
    for (const name of ["outbound", "received"]) {
      const directory = join(path, name);
      mkdirSync(directory, { recursive: true });
      chmodSync(directory, CASE_MODE);
    }
  }
  return paths;
}

function reportLifecycle(action, started) {
  const elapsed = Date.now() - started;
  let result = "missed";
  if (elapsed <= LIFECYCLE_GOAL_MS) {
    result = "met";
  }
  process.stdout.write(
    `Nearby Linux ${action} in ${elapsed}ms; goal ${result}.\n`,
  );
}

function removeCase(directories) {
  const root = resolve(directories.root);
  if (dirname(root) !== resolve(CASE_ROOT)) {
    throw new Error("Nearby Linux case path escaped its owned directory");
  }
  rmSync(root, { force: true, recursive: true });
}

function up() {
  const started = Date.now();
  const { fingerprint, manifest } = inputs();
  const prepared = state();
  assertPrepared(manifest, fingerprint);
  const directories = caseDirectories();
  const environment = {
    ...buildEnvironment(manifest, fingerprint, prepared.context),
    QUICKSHARE_CASE_A_DIR: directories.peerA,
    QUICKSHARE_CASE_B_DIR: directories.peerB,
  };
  dockerCompose(
    ["up", "--detach", "--wait", "--wait-timeout", "30"],
    environment,
  );
  writeFileSync(
    join(ENVIRONMENT_ROOT, "running.json"),
    `${JSON.stringify(directories)}\n`,
  );
  reportLifecycle("startup ready", started);
}

function down() {
  const started = Date.now();
  const { fingerprint, manifest } = inputs();
  const prepared = state();
  const runningPath = join(ENVIRONMENT_ROOT, "running.json");
  let directories = null;
  if (existsSync(runningPath)) {
    directories = JSON.parse(readFileSync(runningPath, "utf8"));
  }
  dockerCompose(
    ["down", "--remove-orphans", "--volumes"],
    buildEnvironment(manifest, fingerprint, prepared.context),
  );
  rmSync(runningPath, { force: true });
  if (directories) {
    removeCase(directories);
  }
  reportLifecycle("teardown ready", started);
}

function selfTestContext() {
  const runningPath = join(ENVIRONMENT_ROOT, "running.json");
  if (!existsSync(runningPath)) {
    throw new Error("Nearby Linux is not running; run nearby-linux-up");
  }
  const { fingerprint, manifest } = inputs();
  const prepared = state();
  assertPrepared(manifest, fingerprint);
  const environment = buildEnvironment(manifest, fingerprint, prepared.context);
  return {
    cases: JSON.parse(readFileSync(runningPath, "utf8")),
    runner: createComposeRunner({
      compose: COMPOSE_PATH,
      docker: process.env.DOCKER ?? "docker",
      environment,
      failureDirectory: FAILURE_REPORTS,
    }),
  };
}

export async function runTimedSelfTest(name, execute, options = {}) {
  const started = Date.now();
  try {
    const evidence = await execute(options.context ?? selfTestContext());
    if (Date.now() - started > SELF_TEST_LIMIT_MS) {
      throw new Error(`Nearby ${name} self-test exceeded its time limit`);
    }
    process.stdout.write(
      `${JSON.stringify({ evidence, schema: 1, suite: name })}\n`,
    );
  } catch (error) {
    recordFailureArtifact(options.failureDirectory ?? FAILURE_REPORTS, {
      events: [{ event: "suite", status: "failed" }],
      gate: "nearby-linux",
      outcome: { kind: "failed" },
      stage: name,
    });
    throw error;
  }
}

function sharingSelfTest() {
  return runTimedSelfTest("sharing", runSharingSelfTest);
}

function sharingActionsSelfTest() {
  return runTimedSelfTest("sharing-actions", runSharingActionsSelfTest);
}

function connectionsSelfTest() {
  return runTimedSelfTest("connections", runConnectionsSelfTest);
}

async function main() {
  const mode = process.argv[2] ?? "validate";
  if (mode === "validate") {
    const { fingerprint } = inputs();
    process.stdout.write(
      `Validated Nearby Linux environment ${fingerprint}.\n`,
    );
    return;
  }
  const handlers = {
    "connections-self-test": connectionsSelfTest,
    down,
    provision,
    "self-test": sharingSelfTest,
    "sharing-actions-self-test": sharingActionsSelfTest,
    up,
  };
  if (!Object.hasOwn(handlers, mode)) {
    throw new Error(`unknown Nearby Linux environment command: ${mode}`);
  }
  await handlers[mode]();
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await main();
}

export { SELF_TEST_LIMIT_MS, buildEnvironment, caseDirectories, digest };
