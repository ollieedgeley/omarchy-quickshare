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
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../../../tools/gates/lib/process.mjs";

const PRIVATE_FILE_MODE = 0o600;

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
