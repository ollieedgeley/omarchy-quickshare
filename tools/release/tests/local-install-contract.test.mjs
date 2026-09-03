import assert from "node:assert/strict";
import {
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

import { installLocal } from "../local-install.mjs";

const temporaryDirectories = new Set();
const EXECUTABLE_MODE = 0o100755;
const RESTART = /^Restart=on-failure$/mu;
const RESTART_DELAY = /^RestartSec=2s$/mu;
const SERVICE_COMMAND =
  /^ExecStart=%h\/\.local\/bin\/omarchy-quickshare --daemon$/mu;
const SIMULATED_SERVICE_COMMAND =
  /^ExecStart=%h\/\.local\/bin\/omarchy-quickshare --daemon --simulate$/mu;
const SERVICE_TARGET = /^WantedBy=default.target$/mu;

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-install-"));
  temporaryDirectories.add(directory);
  return directory;
}

function preparedSource(root) {
  const binary = join(root, "target", "release", "omarchy-quickshare");
  const service = join(
    root,
    "packaging",
    "systemd",
    "omarchy-quickshare.service",
  );
  mkdirSync(join(root, "target", "release"), { recursive: true });
  mkdirSync(join(root, "packaging", "systemd"), { recursive: true });
  writeFileSync(binary, "binary");
  writeFileSync(
    service,
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare --daemon\n",
  );
  return { binary, service };
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
  assert.equal(statSync(paths.binary).mode, EXECUTABLE_MODE);
}

function assertCommands(calls, root) {
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
      args: ["--user", "restart", "omarchy-quickshare.service"],
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
  assertCommands(calls, root);
});

test("user service runs the daemon and restarts after failures", () => {
  const root = process.cwd();
  const service = readFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.service"),
    "utf8",
  );

  assert.match(service, SERVICE_COMMAND);
  assert.match(service, RESTART);
  assert.match(service, RESTART_DELAY);
  assert.match(service, SERVICE_TARGET);
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
  assertCommands(calls, root);
});
