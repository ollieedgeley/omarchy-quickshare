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
import test, { afterEach } from "node:test";

import { installLocal, uninstallLocal } from "../local-install.mjs";

const temporaryDirectories = new Set();
const EXECUTABLE_MODE = 0o100755;
const RESTART = /^Restart=on-failure$/mu;
const RESTART_DELAY = /^RestartSec=2s$/mu;
const SERVICE_COMMAND =
  /^ExecStart=%h\/\.local\/bin\/omarchy-quickshare daemon$/mu;
const SIMULATED_SERVICE_COMMAND =
  /^ExecStart=%h\/\.local\/bin\/omarchy-quickshare daemon --simulate$/mu;
const SERVICE_TARGET = /^WantedBy=default.target$/mu;
const DISCOVERY_TIMEOUT = /^discovery_timeout_secs = 15$/mu;
const RECEIVE_DIRECTORY =
  /^receive_directory = "~\/Downloads\/omarchy-quickshare"$/mu;
const TRANSFER_TIMEOUT = /^transfer_timeout_secs = 120$/mu;
const VISIBILITY_TIMEOUT = /^visibility_timeout_secs = 300$/mu;
const DEFAULT_CONFIG = `discovery_timeout_secs = 15
receive_directory = "~/Downloads/omarchy-quickshare"
transfer_timeout_secs = 120
visibility_timeout_secs = 300
`;

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-install-"));
  temporaryDirectories.add(directory);
  return directory;
}

function preparedSource(root) {
  const binary = join(root, "target", "release", "omarchy-quickshare");
  const config = join(root, "packaging", "systemd", "omarchy-quickshare.toml");
  const service = join(
    root,
    "packaging",
    "systemd",
    "omarchy-quickshare.service",
  );
  mkdirSync(join(root, "target", "release"), { recursive: true });
  mkdirSync(join(root, "packaging", "systemd"), { recursive: true });
  writeFileSync(binary, "binary");
  writeFileSync(config, DEFAULT_CONFIG);
  writeFileSync(
    service,
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare daemon\n",
  );
  return { binary, config, service };
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
    service: join(
      homeDirectory,
      ".config",
      "systemd",
      "user",
      "omarchy-quickshare.service",
    ),
  };
}

function assertInstalled(paths, binary, service) {
  assert.equal(
    readFileSync(paths.binary, "utf8"),
    readFileSync(binary, "utf8"),
  );
  assert.equal(
    readFileSync(paths.service, "utf8"),
    readFileSync(service, "utf8"),
  );
  assert.equal(readFileSync(paths.config, "utf8"), DEFAULT_CONFIG);
  assert.equal(statSync(paths.binary).mode, EXECUTABLE_MODE);
}

function assertCommands(calls, root, action) {
  assert.deepEqual(calls, [
    {
      args: [
        "build",
        "--release",
        "--locked",
        "--package",
        "omarchy-quickshare",
      ],
      command: "cargo",
      options: { cwd: root },
    },
    { args: ["--user", "daemon-reload"], command: "systemctl" },
    {
      args: ["--user", "enable", "omarchy-quickshare.service"],
      command: "systemctl",
    },
    {
      args: ["--user", action, "omarchy-quickshare.service"],
      command: "systemctl",
    },
  ]);
}

function commandRecorder(calls) {
  return (command, args, options) => {
    const call = { args, command };
    if (options) {
      call.options = options;
    }
    calls.push(call);
  };
}

test("local installer builds, installs, and starts its user service", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  const { binary, service } = preparedSource(root);
  const calls = [];

  installLocal({ homeDirectory, root, runCommand: commandRecorder(calls) });

  assertInstalled(installedPaths(homeDirectory), binary, service);
  assertCommands(calls, root, "start");
});

test("user service runs the daemon and restarts after failures", () => {
  const root = process.cwd();
  const service = readFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.service"),
    "utf8",
  );
  const config = readFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.toml"),
    "utf8",
  );

  assert.match(service, SERVICE_COMMAND);
  assert.match(service, RESTART);
  assert.match(service, RESTART_DELAY);
  assert.match(service, SERVICE_TARGET);
  assert.doesNotMatch(service, SIMULATED_SERVICE_COMMAND);
  assert.match(config, DISCOVERY_TIMEOUT);
  assert.match(config, RECEIVE_DIRECTORY);
  assert.match(config, TRANSFER_TIMEOUT);
  assert.match(config, VISIBILITY_TIMEOUT);
});

test("local installer can explicitly enable simulated peers", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  preparedSource(root);
  const calls = [];

  installLocal({
    homeDirectory,
    root,
    runCommand: commandRecorder(calls),
    simulated: true,
  });

  const service = readFileSync(installedPaths(homeDirectory).service, "utf8");
  assert.match(service, SIMULATED_SERVICE_COMMAND);
  assertCommands(calls, root, "start");
});

test("local installer restarts an already installed user service", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  preparedSource(root);
  const first = [];
  const second = [];
  const paths = installedPaths(homeDirectory);

  installLocal({ homeDirectory, root, runCommand: commandRecorder(first) });
  writeFileSync(paths.config, "# user\n");
  installLocal({ homeDirectory, root, runCommand: commandRecorder(second) });

  assert.equal(readFileSync(paths.config, "utf8"), "# user\n");
  assertCommands(second, root, "restart");
});

test("local installer leaves user config when uninstalling owned files", () => {
  const root = temporaryDirectory();
  const homeDirectory = temporaryDirectory();
  preparedSource(root);
  const calls = [];
  installLocal({ homeDirectory, root, runCommand: commandRecorder(calls) });
  const paths = installedPaths(homeDirectory);
  writeFileSync(paths.config, "# user\n");
  calls.length = 0;

  uninstallLocal({ homeDirectory, root, runCommand: commandRecorder(calls) });

  assert.equal(existsSync(paths.binary), false);
  assert.equal(existsSync(paths.service), false);
  assert.equal(readFileSync(paths.config, "utf8"), "# user\n");
  assert.deepEqual(calls, [
    {
      args: ["--user", "stop", "omarchy-quickshare.service"],
      command: "systemctl",
    },
    {
      args: ["--user", "disable", "omarchy-quickshare.service"],
      command: "systemctl",
    },
    { args: ["--user", "daemon-reload"], command: "systemctl" },
  ]);
});
