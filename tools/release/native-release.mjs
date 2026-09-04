import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";
import { readAppVersion } from "./source-allowlist.mjs";
import {
  assertCleanReleaseInputs,
  buildLockedBinary,
  hashFile,
} from "./source-build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DESTINATION = join(ROOT, "dist", "native");
const BINARY_NAME = "omarchy-quickshare";
const EXECUTABLE_MODE = 0o755;
const COMMIT_PATTERN = /^[0-9a-f]{40}$/u;
const CONTROL_PROTOCOL = 3;

export function createNativeRelease({
  destination,
  root = ROOT,
  runCommand = run,
  sourceCommit,
} = {}) {
  if (!COMMIT_PATTERN.test(sourceCommit)) {
    throw new Error("native release needs a full source commit");
  }
  mkdirSync(destination, { recursive: true });
  const built = buildLockedBinary({ root, runCommand });
  const binary = join(destination, BINARY_NAME);
  copyFileSync(built.binary, binary);
  chmodSync(binary, EXECUTABLE_MODE);
  copyFileSync(
    join(root, "packaging", "systemd", `${BINARY_NAME}.service`),
    join(destination, `${BINARY_NAME}.service`),
  );
  copyFileSync(
    join(root, "packaging", "systemd", `${BINARY_NAME}.toml`),
    join(destination, `${BINARY_NAME}.toml`),
  );
  copyFileSync(join(root, "LICENSE"), join(destination, "LICENSE"));
  const version = readAppVersion(root);
  if (built.version !== version) {
    throw new Error("native release version does not match the app package");
  }
  const sha256 = hashFile(binary);
  const meta = {
    controlProtocol: CONTROL_PROTOCOL,
    sha256,
    sourceCommit,
    version,
  };
  writeFileSync(
    join(destination, "version.json"),
    `${JSON.stringify(meta, null, 2)}\n`,
  );
  writeFileSync(join(destination, "SHA256SUMS"), `${sha256}  ${BINARY_NAME}\n`);
  return { binary, sha256, version };
}

function main() {
  assertCleanReleaseInputs();
  const sourceCommit = output("git", ["rev-parse", "HEAD"]);
  if (resolve(DESTINATION) !== DESTINATION) {
    throw new Error("refusing to clean an unexpected release path");
  }
  rmSync(DESTINATION, { force: true, recursive: true });
  const result = createNativeRelease({
    destination: DESTINATION,
    sourceCommit,
  });
  process.stdout.write(`Native ${result.version} for ${sourceCommit}.\n`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
