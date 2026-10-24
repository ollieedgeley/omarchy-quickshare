import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  closeSync,
  existsSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../tools/gates/lib/process.mjs";

import { contextPath } from "./context.mjs";

const PRIVATE_FILE_MODE = 0o600;
const IMAGE_ID_PATTERN = /^sha256:[0-9a-f]{64}$/u;

export function preparedImage(statePath, cacheRoot, configuration) {
  const { dockerfile, fingerprint, manifest } = configuration;
  try {
    const prepared = JSON.parse(readFileSync(statePath, "utf8"));
    if (
      prepared?.fingerprint !== fingerprint ||
      typeof prepared.context !== "string" ||
      !IMAGE_ID_PATTERN.test(prepared.imageId) ||
      resolve(prepared.context) !==
        resolve(contextPath(cacheRoot, fingerprint)) ||
      !statSync(prepared.context).isDirectory() ||
      readFileSync(join(prepared.context, "Dockerfile"), "utf8") !== dockerfile
    ) {
      return false;
    }
    const result = run(
      process.env.DOCKER ?? "docker",
      ["image", "inspect", "--format", "{{json .}}", manifest.image],
      { allowFailure: true, capture: true, quiet: true },
    );
    if (result.status !== 0) {
      return false;
    }
    const image = JSON.parse(result.stdout);
    return (
      image.Id === prepared.imageId &&
      image.Config?.Labels?.["org.omarchy-quickshare.fingerprint"] ===
        fingerprint
    );
  } catch {
    return false;
  }
}

export function publishPreparedState(statePath, configuration, context) {
  const { fingerprint, manifest } = configuration;
  const imageId = output(process.env.DOCKER ?? "docker", [
    "image",
    "inspect",
    "--format",
    "{{.Id}}",
    manifest.image,
  ]);
  writeFileSync(
    statePath,
    `${JSON.stringify({ context, fingerprint, imageId }, null, 2)}\n`,
  );
  process.stdout.write(`Prepared Nearby Linux image ${imageId}.\n`);
}

export function withProvisionLock(directory, provision) {
  mkdirSync(directory, { recursive: true });
  const path = join(directory, "provision.lock");
  const descriptor = openSync(path, "a", PRIVATE_FILE_MODE);
  try {
    // Flock and this parent share the open file description. Keep the parent
    // Descriptor open through publication; never unlink the lock inode.
    const result = spawnSync("flock", ["--exclusive", "--nonblock", "3"], {
      stdio: ["ignore", "inherit", "inherit", descriptor],
    });
    if (result.error) {
      throw result.error;
    }
    if (result.status !== 0) {
      throw new Error(`Nearby Linux provision lock unavailable: ${path}`);
    }
    return provision(descriptor);
  } finally {
    closeSync(descriptor);
  }
}

export function capacityCommand(
  compose,
  args,
  { environment, descriptor = null },
) {
  const admission = fileURLToPath(
    new URL("../desktop/resource_budget.py", import.meta.url),
  );
  let prefix = ["run", "peer", dirname(compose), "1", "512", "--"];
  if (args[0] === "build") {
    prefix = ["reference-build"];
  }
  const command = [
    process.env.DOCKER ?? "docker",
    "compose",
    "--file",
    compose,
    ...args,
  ];
  const stdio = ["inherit", "inherit", "inherit"];
  if (descriptor !== null) {
    stdio.push(descriptor);
  }
  const result = spawnSync("python3", [admission, ...prefix, ...command], {
    env: environment,
    stdio,
  });
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0) {
    throw new Error(`Capacity-controlled command exited with ${result.status}`);
  }
}

function digest(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

export function downloadVerified(url, expected, destination) {
  if (existsSync(destination) && digest(destination) === expected) {
    return;
  }
  const temporary = `${destination}.temporary`;
  rmSync(temporary, { force: true });
  mkdirSync(dirname(destination), { recursive: true });
  run("curl", ["--fail", "--location", "--output", temporary, url]);
  if (digest(temporary) !== expected) {
    rmSync(temporary, { force: true });
    throw new Error(`Nearby Linux download SHA-256 mismatch: ${url}`);
  }
  renameSync(temporary, destination);
}
