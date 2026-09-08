import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "vitest";

import { installLocal, uninstallLocal } from "../local-install.mjs";

const temporaryDirectories = new Set();
const EXECUTABLE_MODE = 0o100755;
const SIMULATED_SERVICE_COMMAND =
  /^ExecStart=%h\/\.local\/bin\/omarchy-quickshare-daemon --simulate$/mu;
const DEFAULT_CONFIG = `# Default local endpoint configuration.
consent_timeout_secs = 300
discoverable = false
discovery_timeout_secs = 15
read_clipboard_on_select = false
receive_directory = "~/Downloads/omarchy-quickshare"
transfer_timeout_secs = 120
`;

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-install-"));
  temporaryDirectories.add(directory);
  return directory;
}

function preparedSource(root) {
  const binary = join(root, "target", "release", "omarchy-quickshare");
  const daemon = join(root, "target", "release", "omarchy-quickshare-daemon");
  const config = join(root, "packaging", "systemd", "omarchy-quickshare.toml");
  const service = join(
    root,
    "packaging",
    "systemd",
    "omarchy-quickshare.service",
  );
  mkdirSync(join(root, "target", "release"), { recursive: true });
  mkdirSync(join(root, "packaging", "systemd"), { recursive: true });
  writeFileSync(binary, "cli");
  writeFileSync(daemon, "daemon");
  writeFileSync(config, DEFAULT_CONFIG);
  for (const [crate, name] of [
    ["app", "omarchy-quickshare"],
    ["cli", "omarchy-quickshare-cli"],
  ]) {
    mkdirSync(join(root, "crates", crate), { recursive: true });
    writeFileSync(
      join(root, "crates", crate, "Cargo.toml"),
      `[package]\nname = "${name}"\nversion = "0.0.0"\n`,
    );
  }
  writeFileSync(
    service,
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare-daemon\n",
  );
  return { binary, config, daemon, service };
}

function runInstallCommand(command, args) {
  if (args[0] === "protocol-version" || args[0] === "--protocol-version") {
    return { stdout: "7" };
  }
  return null;
}

afterEach(() => {
  for (const directory of temporaryDirectories) {
    rmSync(directory, { force: true, recursive: true });
  }
  temporaryDirectories.clear();
});

function installedPaths(homeDirectory) {
  return {
    binary: join(homeDirectory, ".local", "bin", "omarchy-quickshare"),
    config: join(homeDirectory, ".config", "omarchy-quickshare", "config.toml"),
    daemon: join(homeDirectory, ".local", "bin", "omarchy-quickshare-daemon"),
    service: join(
      homeDirectory,
      ".config",
      "systemd",
      "user",
      "omarchy-quickshare.service",
    ),
  };
}

function assertInstalled(paths, { binary, daemon, service }) {
  assert.equal(
    readFileSync(paths.binary, "utf8"),
    readFileSync(binary, "utf8"),
  );
  assert.equal(
    readFileSync(paths.daemon, "utf8"),
    readFileSync(daemon, "utf8"),
  );
  assert.equal(
    readFileSync(paths.service, "utf8"),
    readFileSync(service, "utf8"),
  );
  assert.equal(readFileSync(paths.config, "utf8"), DEFAULT_CONFIG);
  assert.equal(statSync(paths.binary).mode, EXECUTABLE_MODE);
  assert.equal(statSync(paths.daemon).mode, EXECUTABLE_MODE);
}

test("local install copies fixtures with simulated build and service", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  const source = preparedSource(root);

  installLocal({ homeDirectory, root, runCommand: runInstallCommand });

  assertInstalled(installedPaths(homeDirectory), source);
});

test("local install writes the explicitly simulated service", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  preparedSource(root);

  installLocal({
    homeDirectory,
    root,
    runCommand: runInstallCommand,
    simulated: true,
  });

  const service = readFileSync(installedPaths(homeDirectory).service, "utf8");
  assert.match(service, SIMULATED_SERVICE_COMMAND);
});

test("local reinstall preserves config with simulated commands", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  preparedSource(root);
  const paths = installedPaths(homeDirectory);

  installLocal({ homeDirectory, root, runCommand: runInstallCommand });
  writeFileSync(paths.config, "# user\n");
  installLocal({ homeDirectory, root, runCommand: runInstallCommand });

  assert.equal(readFileSync(paths.config, "utf8"), "# user\n");
});

test("local uninstall preserves config with simulated service commands", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  preparedSource(root);
  installLocal({ homeDirectory, root, runCommand: runInstallCommand });
  const paths = installedPaths(homeDirectory);
  writeFileSync(paths.config, "# user\n");

  uninstallLocal({ homeDirectory, root, runCommand: runInstallCommand });

  assert.equal(existsSync(paths.binary), false);
  assert.equal(existsSync(paths.daemon), false);
  assert.equal(existsSync(paths.service), false);
  assert.equal(readFileSync(paths.config, "utf8"), "# user\n");
});
