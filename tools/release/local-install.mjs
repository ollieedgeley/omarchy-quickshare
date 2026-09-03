import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  readFileSync,
  renameSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../gates/lib/process.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const EXECUTABLE_MODE = 0o755;
const SERVICE_NAME = "omarchy-quickshare.service";
const SERVICE_COMMAND_PATTERN = /^ExecStart=.* --daemon$/mu;

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

function installService(paths, simulated) {
  if (!simulated) {
    copyFileSync(paths.serviceSource, paths.serviceTarget);
    return;
  }
  const service = readFileSync(paths.serviceSource, "utf8");
  if (!SERVICE_COMMAND_PATTERN.test(service)) {
    throw new Error("service does not contain the expected daemon command");
  }
  writeFileSync(
    paths.serviceTarget,
    service.replace(SERVICE_COMMAND_PATTERN, "$& --simulate"),
  );
}

export function installLocal({
  homeDirectory = homedir(),
  root = ROOT,
  runCommand = run,
  simulated = false,
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
  installService(paths, simulated);
  runCommand("systemctl", ["--user", "daemon-reload"]);
  runCommand("systemctl", ["--user", "enable", SERVICE_NAME]);
  runCommand("systemctl", ["--user", "restart", SERVICE_NAME]);
}

function main() {
  const cliArguments = process.argv.slice(2);
  if (
    cliArguments.length > 1 ||
    (cliArguments.length === 1 && cliArguments[0] !== "--simulate")
  ) {
    throw new Error("usage: local-install.mjs [--simulate]");
  }
  installLocal({ simulated: cliArguments[0] === "--simulate" });
  process.stdout.write("Installed and started Omarchy Quick Share.\n");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
