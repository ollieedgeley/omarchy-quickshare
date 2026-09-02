import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../tools/gates/lib/process.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "oracle");
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.toolchain");
const STATE_PATH = join(ROOT, ".cache", "test-env", "oracle", "image.json");
const CONTAINER = "omarchy-quickshare-oracle";
const START_LIMIT_MS = 60_000;
const START_GOAL_MS = 30_000;

function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

export function environmentFingerprint(manifestSource, dockerfileSource) {
  return digest(`${manifestSource}\0${dockerfileSource}`);
}

export function validateEnvironment(manifestSource, dockerfileSource) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) throw new Error("unsupported oracle schema");
  if (!/^[a-z0-9./-]+:[0-9]{4}-[0-9]{2}-[0-9]{2}$/.test(manifest.image)) {
    throw new Error("oracle image must use a dated immutable tag");
  }
  if (!/^debian@sha256:[0-9a-f]{64}$/.test(manifest.base)) {
    throw new Error("oracle base image must use a SHA-256 digest");
  }
  if (!/^20[0-9]{6}T000000Z$/.test(manifest.debianSnapshot)) {
    throw new Error("oracle Debian snapshot must identify a UTC day");
  }
  if (!/^[0-9]+\.[0-9]+\.[0-9]+$/.test(manifest.bazel?.version)) {
    throw new Error("oracle Bazel version must be exact");
  }
  if (
    !manifest.bazel.url.startsWith("https://releases.bazel.build/") ||
    !manifest.bazel.url.includes(`/bazel-${manifest.bazel.version}-`)
  ) {
    throw new Error("oracle Bazel URL does not match its version");
  }
  if (!/^[0-9a-f]{64}$/.test(manifest.bazel.sha256)) {
    throw new Error("oracle Bazel binary must have a SHA-256");
  }
  if (
    !Array.isArray(manifest.packages) ||
    manifest.packages.length === 0 ||
    new Set(manifest.packages).size !== manifest.packages.length ||
    [...manifest.packages].sort().join("\0") !== manifest.packages.join("\0")
  ) {
    throw new Error("oracle packages must be a non-empty sorted unique list");
  }
  const requiredFragments = [
    `FROM ${manifest.base}`,
    `ARG DEBIAN_SNAPSHOT=${manifest.debianSnapshot}`,
    `ARG BAZEL_URL=${manifest.bazel.url}`,
    `ARG BAZEL_SHA256=${manifest.bazel.sha256}`,
    'io.omarchy-quickshare.environment="${ENVIRONMENT_FINGERPRINT}"',
  ];
  for (const fragment of requiredFragments) {
    if (!dockerfileSource.includes(fragment)) {
      throw new Error(`oracle Dockerfile lacks manifest value: ${fragment}`);
    }
  }
  for (const packageName of manifest.packages) {
    const packageLine = `      ${packageName} \\`;
    if (!dockerfileSource.split("\n").includes(packageLine)) {
      throw new Error(`oracle Dockerfile lacks package ${packageName}`);
    }
  }
  return manifest;
}

function inputs() {
  const manifestSource = readFileSync(MANIFEST_PATH, "utf8");
  const dockerfileSource = readFileSync(DOCKERFILE_PATH, "utf8");
  return {
    manifest: validateEnvironment(manifestSource, dockerfileSource),
    fingerprint: environmentFingerprint(manifestSource, dockerfileSource),
  };
}

function docker(args, options = {}) {
  return run(process.env.DOCKER ?? "docker", args, options);
}

function dockerOutput(args, allowFailure = false) {
  const result = docker(args, { capture: true, quiet: true, allowFailure });
  return { status: result.status, stdout: result.stdout.trim() };
}

function inspectContainer() {
  return dockerOutput(["container", "inspect", CONTAINER], true).status === 0;
}

function runningContainer() {
  return (
    dockerOutput(
      ["container", "inspect", "--format", "{{.State.Running}}", CONTAINER],
      true,
    ).stdout === "true"
  );
}

function assertPrepared(manifest, fingerprint) {
  const image = dockerOutput(["image", "inspect", manifest.image], true);
  if (image.status !== 0) {
    throw new Error("oracle image is missing; run `make oracle-provision`");
  }
  const label = dockerOutput([
    "image",
    "inspect",
    "--format",
    '{{index .Config.Labels "io.omarchy-quickshare.environment"}}',
    manifest.image,
  ]).stdout;
  if (label !== fingerprint) {
    throw new Error(
      "oracle image inputs changed; rerun `make oracle-provision`",
    );
  }
}

function provision(manifest, fingerprint) {
  mkdirSync(dirname(STATE_PATH), { recursive: true });
  docker([
    "buildx",
    "build",
    "--load",
    "--provenance=false",
    "--file",
    DOCKERFILE_PATH,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${fingerprint}`,
    "--tag",
    manifest.image,
    ROOT,
  ]);
  assertPrepared(manifest, fingerprint);
  const imageId = dockerOutput([
    "image",
    "inspect",
    "--format",
    "{{.Id}}",
    manifest.image,
  ]).stdout;
  writeFileSync(
    STATE_PATH,
    `${JSON.stringify({ fingerprint, image: manifest.image, imageId }, null, 2)}\n`,
  );
  process.stdout.write(`Prepared oracle image ${imageId}.\n`);
}

function enforceLifecycle(name, started) {
  const elapsed = Date.now() - started;
  if (elapsed > START_LIMIT_MS) {
    throw new Error(`${name} took ${elapsed}ms; lifecycle limit is 60000ms`);
  }
  const goal = elapsed <= START_GOAL_MS ? "met" : "missed";
  process.stdout.write(
    `${name} ready in ${elapsed}ms; 30000ms goal ${goal}.\n`,
  );
}

function up(manifest, fingerprint) {
  const started = Date.now();
  assertPrepared(manifest, fingerprint);
  if (inspectContainer() && !runningContainer()) {
    docker(["container", "rm", CONTAINER]);
  }
  if (!runningContainer()) {
    docker([
      "run",
      "--detach",
      "--name",
      CONTAINER,
      "--read-only",
      "--tmpfs",
      "/work/home:rw,nosuid,nodev,size=64m",
      manifest.image,
      "sleep",
      "infinity",
    ]);
  }
  docker(["exec", CONTAINER, "sh", "-ceu", "test -w /work/home"]);
  enforceLifecycle("oracle startup", started);
}

function down() {
  const started = Date.now();
  if (inspectContainer()) docker(["container", "rm", "--force", CONTAINER]);
  enforceLifecycle("oracle teardown", started);
}

function selfTest(manifest) {
  if (!runningContainer())
    throw new Error("oracle is not running; run `make oracle-up`");
  const expected = `bazel ${manifest.bazel.version}`;
  const actual = dockerOutput(["exec", CONTAINER, "bazel", "--version"]).stdout;
  if (actual !== expected) {
    throw new Error(`expected ${expected}, received ${actual}`);
  }
  docker([
    "exec",
    CONTAINER,
    "sh",
    "-ceu",
    "clang --version >/dev/null && clang++ --version >/dev/null && python3 --version >/dev/null",
  ]);
  process.stdout.write("Oracle toolchain self-test passed.\n");
}

async function main() {
  const mode = process.argv[2] ?? "validate";
  const { manifest, fingerprint } = inputs();
  if (mode === "validate") {
    process.stdout.write(`Validated oracle environment ${fingerprint}.\n`);
  } else if (mode === "provision") {
    provision(manifest, fingerprint);
  } else if (mode === "up") {
    up(manifest, fingerprint);
  } else if (mode === "down") {
    down();
  } else if (mode === "self-test") {
    selfTest(manifest);
  } else {
    throw new Error(`unknown oracle environment action: ${mode}`);
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
