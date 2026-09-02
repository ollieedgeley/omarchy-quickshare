import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../tools/gates/lib/process.mjs";

const { recordFailureArtifact } = await import(
  new URL("../../../tools/gates/lib/failure-artifact.mjs", import.meta.url)
);

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const SOURCE = join(CACHE, "sources", "trees", "nearshare");
const MANIFEST_PATH = join(DIRECTORY, "environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.toolchain");
const BASE_IMAGE_PATTERN = /^debian@sha256:[0-9a-f]{64}$/u;
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const SNAPSHOT_PATTERN = /^\d{8}T\d{6}Z$/u;
const VERSION_PATTERN = /^\d[\dA-Za-z.+:~-]*$/u;
const LIFECYCLE_GOAL_MS = 30_000;

function docker(args, options = {}) {
  return run("docker", args, options);
}

function fingerprint(manifestSource, dockerfile) {
  return createHash("sha256")
    .update(manifestSource)
    .update("\0")
    .update(dockerfile)
    .digest("hex");
}

export function validateEnvironment(manifestSource, dockerfile) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) {
    throw new Error("unsupported NearShare environment schema");
  }
  if (!BASE_IMAGE_PATTERN.test(manifest.base)) {
    throw new Error("NearShare base image must use a SHA-256 digest");
  }
  if (!SNAPSHOT_PATTERN.test(manifest.debianSnapshot)) {
    throw new Error("NearShare Debian snapshot must be timestamped");
  }
  if (!REVISION_PATTERN.test(manifest.source.revision)) {
    throw new Error("NearShare revision must be a full commit");
  }
  for (const [name, version] of Object.entries(manifest.packages)) {
    if (
      !VERSION_PATTERN.test(version) ||
      !dockerfile.includes(`${name}=${version}`)
    ) {
      throw new Error(`NearShare package is not pinned: ${name}`);
    }
  }
  for (const value of [
    manifest.base,
    manifest.debianSnapshot,
    "ENVIRONMENT_FINGERPRINT",
  ]) {
    if (!dockerfile.includes(value)) {
      throw new Error(`NearShare Dockerfile lacks pin: ${value}`);
    }
  }
  return manifest;
}

export function environmentFingerprint(manifestSource, dockerfile) {
  return fingerprint(manifestSource, dockerfile);
}

function inputs() {
  const manifestSource = readFileSync(MANIFEST_PATH, "utf8");
  const dockerfile = readFileSync(DOCKERFILE_PATH, "utf8");
  return {
    dockerfile,
    fingerprint: fingerprint(manifestSource, dockerfile),
    manifest: validateEnvironment(manifestSource, dockerfile),
  };
}

function imageFingerprint(image) {
  return output("docker", [
    "image",
    "inspect",
    "--format",
    '{{index .Config.Labels "io.omarchy-quickshare.environment"}}',
    image,
  ]);
}

function assertPrepared(manifest, expected) {
  const result = docker(["image", "inspect", manifest.image], {
    allowFailure: true,
    capture: true,
    quiet: true,
  });
  if (result.status !== 0 || imageFingerprint(manifest.image) !== expected) {
    throw new Error("NearShare image is stale; run make nearshare-provision");
  }
}

function provision() {
  const { manifest, fingerprint: expected } = inputs();
  docker([
    "build",
    "--file",
    DOCKERFILE_PATH,
    "--build-arg",
    `DEBIAN_SNAPSHOT=${manifest.debianSnapshot}`,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${expected}`,
    "--tag",
    manifest.image,
    DIRECTORY,
  ]);
  assertPrepared(manifest, expected);
  process.stdout.write(`Prepared NearShare peer ${expected}.\n`);
}

function removeContainer(name) {
  docker(["container", "rm", "--force", name], {
    allowFailure: true,
    capture: true,
    quiet: true,
  });
}

function reportLifecycle(action, started) {
  const elapsed = Date.now() - started;
  let result = "missed";
  if (elapsed <= LIFECYCLE_GOAL_MS) {
    result = "met";
  }
  process.stdout.write(
    `NearShare ${action} in ${elapsed}ms; ` +
      `${LIFECYCLE_GOAL_MS}ms goal ${result}.\n`,
  );
}

function up() {
  const started = Date.now();
  const { manifest, fingerprint: expected } = inputs();
  assertPrepared(manifest, expected);
  if (!existsSync(join(SOURCE, "tests", "test_loopback.py"))) {
    throw new Error("NearShare source is missing; run make sources-fetch");
  }
  removeContainer(manifest.container);
  docker([
    "run",
    "--detach",
    "--name",
    manifest.container,
    "--network=host",
    "--read-only",
    "--tmpfs",
    "/work:rw,nosuid,nodev,size=128m",
    "--volume",
    `${SOURCE}:/source:ro`,
    manifest.image,
    "sleep",
    "infinity",
  ]);
  docker([
    "exec",
    manifest.container,
    "sh",
    "-ceu",
    "cp -a /source/. /work/nearshare/; " +
      "mkdir -p /work/home /work/runtime; chmod 700 /work/runtime; " +
      "bash /work/nearshare/tools/genproto.sh python3 >/dev/null",
  ]);
  reportLifecycle("startup ready", started);
}

function selfTest() {
  try {
    const { manifest } = inputs();
    const result = docker(
      ["exec", manifest.container, "python3", "-m", "tests.test_loopback"],
      { capture: true, quiet: true },
    );
    const evidence = [
      "PIN matched on both sides:",
      "2 files received byte-identical",
      "mDNS: discovered own advertisement",
      "LOOPBACK TEST PASSED",
    ];
    if (evidence.some((marker) => !result.stdout.includes(marker))) {
      throw new Error("NearShare self-test lacks required transfer evidence");
    }
    if (result.stdout.includes("mDNS skipped")) {
      throw new Error("NearShare self-test skipped mDNS discovery");
    }
    process.stdout.write(
      '{"schema":1,"peer":"nearshare","roles":["sender","receiver"],' +
        '"pinMatch":true,"payloadMatch":true,"mdns":true}\n',
    );
  } catch (error) {
    recordFailureArtifact(join(ROOT, "reports", "failures"), {
      events: [{ event: "loopback", status: "failed" }],
      gate: "nearshare",
      outcome: { kind: "failed" },
      stage: "loopback-evidence",
    });
    throw error;
  }
}

function down() {
  const started = Date.now();
  const { manifest } = inputs();
  removeContainer(manifest.container);
  reportLifecycle("teardown ready", started);
}

function main() {
  const mode = process.argv[2] ?? "validate";
  if (mode === "validate") {
    const { fingerprint: expected } = inputs();
    process.stdout.write(`Validated NearShare environment ${expected}.\n`);
    return;
  }
  const handlers = { down, provision, "self-test": selfTest, up };
  if (!handlers[mode]) {
    throw new Error(`unknown NearShare environment command: ${mode}`);
  }
  handlers[mode]();
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
