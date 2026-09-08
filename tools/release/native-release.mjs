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
  buildLockedBinaries,
  hashFile,
} from "./source-build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const DESTINATION = join(ROOT, "dist", "native");
const BINARY_NAME = "omarchy-quickshare";
const EXECUTABLE_MODE = 0o755;
const COMMIT_PATTERN = /^[0-9a-f]{40}$/u;

function copyComponents(built, destination) {
  const components = {};
  for (const [role, component] of Object.entries(built.components)) {
    const binary = join(destination, component.file);
    copyFileSync(component.binary, binary);
    chmodSync(binary, EXECUTABLE_MODE);
    const sha256 = hashFile(binary);
    if (sha256 !== component.sha256) {
      throw new Error(`${role} binary changed during native release creation`);
    }
    components[role] = { file: component.file, sha256 };
  }
  return components;
}

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
  const built = buildLockedBinaries({ root, runCommand });
  const components = copyComponents(built, destination);
  for (const file of [`${BINARY_NAME}.service`, `${BINARY_NAME}.toml`]) {
    copyFileSync(
      join(root, "packaging", "systemd", file),
      join(destination, file),
    );
  }
  copyFileSync(join(root, "LICENSE"), join(destination, "LICENSE"));
  const version = readAppVersion(root);
  if (built.version !== version) {
    throw new Error("native release version does not match the app package");
  }
  const meta = {
    components,
    controlProtocol: built.controlProtocol,
    schemaVersion: 2,
    sourceCommit,
    version,
  };
  writeFileSync(
    join(destination, "version.json"),
    `${JSON.stringify(meta, null, 2)}\n`,
  );
  writeFileSync(
    join(destination, "SHA256SUMS"),
    Object.values(components)
      .map(({ file, sha256 }) => `${sha256}  ${file}\n`)
      .join(""),
  );
  return { components, controlProtocol: built.controlProtocol, version };
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
