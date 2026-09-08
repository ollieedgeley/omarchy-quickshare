import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../gates/lib/process.mjs";
import { buildLockedBinaries } from "./source-build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const EXECUTABLE_MODE = 0o755;
const SERVICE_NAME = "omarchy-quickshare.service";
const SERVICE_COMMAND_PATTERN =
  /^ExecStart=%h\/\.local\/bin\/omarchy-quickshare-daemon$/mu;

function installationPaths(root, homeDirectory) {
  return {
    binaries: ["omarchy-quickshare", "omarchy-quickshare-daemon"].map(
      (name) => ({
        source: join(root, "target", "release", name),
        target: join(homeDirectory, ".local", "bin", name),
      }),
    ),
    configSource: join(root, "packaging", "systemd", "omarchy-quickshare.toml"),
    configTarget: join(
      homeDirectory,
      ".config",
      "omarchy-quickshare",
      "config.toml",
    ),
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

function installConfig(paths) {
  mkdirSync(dirname(paths.configTarget), { recursive: true });
  if (!existsSync(paths.configTarget)) {
    copyFileSync(paths.configSource, paths.configTarget);
  }
}

function unlinkIfExists(path) {
  if (existsSync(path)) {
    unlinkSync(path);
  }
}

export function installLocal({
  homeDirectory = homedir(),
  root = ROOT,
  runCommand = run,
  simulated = false,
} = {}) {
  const paths = installationPaths(root, homeDirectory);
  const replacing = paths.binaries.some(({ target }) => existsSync(target));
  buildLockedBinaries({ root, runCommand });
  mkdirSync(dirname(paths.serviceTarget), { recursive: true });
  for (const { source, target } of paths.binaries) {
    mkdirSync(dirname(target), { recursive: true });
    copyFileSync(source, `${target}.installing`);
    chmodSync(`${target}.installing`, EXECUTABLE_MODE);
  }
  for (const { target } of paths.binaries) {
    renameSync(`${target}.installing`, target);
  }
  installService(paths, simulated);
  installConfig(paths);
  let action = "start";
  if (replacing) {
    action = "restart";
  }
  runCommand("systemctl", ["--user", "daemon-reload"]);
  runCommand("systemctl", ["--user", "enable", SERVICE_NAME]);
  runCommand("systemctl", ["--user", action, SERVICE_NAME]);
}

export function uninstallLocal({
  homeDirectory = homedir(),
  root = ROOT,
  runCommand = run,
} = {}) {
  const paths = installationPaths(root, homeDirectory);
  runCommand("systemctl", ["--user", "stop", SERVICE_NAME]);
  runCommand("systemctl", ["--user", "disable", SERVICE_NAME]);
  for (const { target } of paths.binaries) {
    unlinkIfExists(target);
  }
  unlinkIfExists(paths.serviceTarget);
  runCommand("systemctl", ["--user", "daemon-reload"]);
}

function main() {
  const cliArguments = process.argv.slice(2);
  if (cliArguments.length === 1 && cliArguments[0] === "--uninstall") {
    uninstallLocal();
    process.stdout.write("Stopped and removed Omarchy Quick Share.\n");
    return;
  }
  if (
    cliArguments.length > 1 ||
    (cliArguments.length === 1 && cliArguments[0] !== "--simulate")
  ) {
    throw new Error("usage: local-install.mjs [--simulate|--uninstall]");
  }
  installLocal({ simulated: cliArguments[0] === "--simulate" });
  process.stdout.write("Installed and started Omarchy Quick Share.\n");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
