import { chmodSync, copyFileSync, mkdirSync, renameSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../gates/lib/process.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const EXECUTABLE_MODE = 0o755;
const SERVICE_NAME = "omarchy-quickshare.service";

function installationPaths(root, homeDirectory) {
  const binaryName = "omarchy-quickshare";
  return {
    binarySource: join(root, "target", "release", binaryName),
    binaryTarget: join(homeDirectory, ".local", "bin", binaryName),
    serviceSource: join(root, "packaging", "systemd", SERVICE_NAME),
    serviceTarget: join(
      homeDirectory,
      ".config",
      "systemd",
      "user",
      SERVICE_NAME,
    ),
  };
}

export function installLocal({
  homeDirectory = homedir(),
  root = ROOT,
  runCommand = run,
} = {}) {
  const paths = installationPaths(root, homeDirectory);
  runCommand(
    "cargo",
    ["build", "--release", "--locked", "--package", "omarchy-quickshare"],
    { cwd: root },
  );
  mkdirSync(dirname(paths.binaryTarget), { recursive: true });
  mkdirSync(dirname(paths.serviceTarget), { recursive: true });
  const stagedBinary = `${paths.binaryTarget}.installing`;
  copyFileSync(paths.binarySource, stagedBinary);
  chmodSync(stagedBinary, EXECUTABLE_MODE);
  renameSync(stagedBinary, paths.binaryTarget);
  copyFileSync(paths.serviceSource, paths.serviceTarget);
  runCommand("systemctl", ["--user", "daemon-reload"]);
  runCommand("systemctl", ["--user", "enable", SERVICE_NAME]);
  runCommand("systemctl", ["--user", "restart", SERVICE_NAME]);
}

function main() {
  installLocal();
  process.stdout.write("Installed and started Omarchy Quick Share.\n");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
