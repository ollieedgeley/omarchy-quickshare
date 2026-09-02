import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const { recordFailureArtifact } = await import(
  new URL("../../../tools/gates/lib/failure-artifact.mjs", import.meta.url)
);

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const SOURCE = join(CACHE, "sources", "trees", "python-dbusmock");
const MANIFEST_PATH = join(DIRECTORY, "dbus-environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.dbus");
const LIFECYCLE_TIMEOUT_MS = 60_000;
const BASE_IMAGE_PATTERN = /^debian@sha256:[0-9a-f]{64}$/u;
const SNAPSHOT_PATTERN = /^\d{8}T\d{6}Z$/u;
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const VERSION_PATTERN = /^\d+\.\d+(?:\.\d+)?$/u;

function runOptions(options) {
  const result = {
    encoding: "utf8",
    env: process.env,
    stdio: "inherit",
  };
  if (options.env) {
    result.env = options.env;
  }
  if (options.capture) {
    result.stdio = "pipe";
  }
  return result;
}

function run(command, args, options = {}) {
  if (!options.quiet) {
    process.stdout.write(`+ ${command} ${args.join(" ")}\n`);
  }
  const result = spawnSync(command, args, runOptions(options));
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0 && !options.allowFailure) {
    let detail = "";
    if (options.capture) {
      detail = `\n${result.stdout ?? ""}${result.stderr ?? ""}`;
    }
    throw new Error(`${command} exited with ${result.status}${detail}`);
  }
  return result;
}

export function validateEnvironment(manifestSource, dockerfile) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) {
    throw new Error("unsupported D-Bus schema");
  }
  if (!BASE_IMAGE_PATTERN.test(manifest.base)) {
    throw new Error("D-Bus base image must use a SHA-256 digest");
  }
  if (!SNAPSHOT_PATTERN.test(manifest.debianSnapshot)) {
    throw new Error("D-Bus Debian snapshot must be timestamped");
  }
  if (!REVISION_PATTERN.test(manifest.source.revision)) {
    throw new Error("python-dbusmock revision must be a full commit");
  }
  if (new Set(manifest.clients).size !== manifest.clients.length) {
    throw new Error("D-Bus client list contains duplicates");
  }
  for (const client of [...manifest.clients, "python"]) {
    if (!VERSION_PATTERN.test(manifest.versions[client])) {
      throw new Error(`D-Bus client ${client} needs an exact version`);
    }
  }
  for (const value of [
    manifest.base,
    manifest.debianSnapshot,
    ...manifest.packages,
    "ENVIRONMENT_FINGERPRINT",
  ]) {
    if (!dockerfile.includes(value)) {
      throw new Error(
        `D-Bus Dockerfile lacks required pin or client: ${value}`,
      );
    }
  }
  return manifest;
}

export function environmentFingerprint(manifestSource, dockerfile) {
  return createHash("sha256")
    .update(manifestSource)
    .update("\0")
    .update(dockerfile)
    .digest("hex");
}

function inputs() {
  const manifestSource = readFileSync(MANIFEST_PATH, "utf8");
  const dockerfile = readFileSync(DOCKERFILE_PATH, "utf8");
  return {
    manifest: validateEnvironment(manifestSource, dockerfile),
    manifestSource,
    dockerfile,
  };
}

function provision() {
  const { manifest, manifestSource, dockerfile } = inputs();
  const pythonVersion = manifest.versions.python;
  if (!existsSync(join(SOURCE, "dbusmock", "templates", "bluez5.py"))) {
    throw new Error(
      "pinned python-dbusmock source is missing; run make sources-fetch",
    );
  }
  run("docker", [
    "build",
    "--file",
    DOCKERFILE_PATH,
    "--build-arg",
    `DEBIAN_SNAPSHOT=${manifest.debianSnapshot}`,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${environmentFingerprint(
      manifestSource,
      dockerfile,
    )}`,
    "--tag",
    manifest.image,
    DIRECTORY,
  ]);
  run("docker", [
    "run",
    "--rm",
    "--network=none",
    "--volume",
    `${SOURCE}:/source:ro`,
    "--env",
    "PYTHONPATH=/source",
    manifest.image,
    "sh",
    "-ceu",
    [
      "python3 -c 'import dbus, dbusmock, gi, packaging'",
      `test "$(bluetoothctl --version)" = ` +
        `'bluetoothctl: ${manifest.versions.bluetoothctl}'`,
      `test "$(nmcli --version)" = ` +
        `'nmcli tool, version ${manifest.versions.nmcli}'`,
      `test "$(python3 --version)" = 'Python ${pythonVersion}'`,
    ].join(" && "),
  ]);
  process.stdout.write("Prepared pinned private D-Bus environment.\n");
}

function containerExists(name) {
  return (
    run("docker", ["container", "inspect", name], {
      allowFailure: true,
      capture: true,
      quiet: true,
    }).status === 0
  );
}

function up() {
  const started = performance.now();
  const { manifest } = inputs();
  if (containerExists(manifest.container)) {
    throw new Error("D-Bus environment is already running");
  }
  run("docker", [
    "run",
    "--detach",
    "--name",
    manifest.container,
    "--network=none",
    "--read-only",
    "--tmpfs",
    "/tmp:rw,nosuid,nodev,size=64m",
    "--tmpfs",
    "/run:rw,nosuid,nodev,size=16m",
    "--volume",
    `${SOURCE}:/source:ro`,
    "--env",
    "PYTHONPATH=/source",
    manifest.image,
    "sleep",
    "infinity",
  ]);
  run("docker", [
    "exec",
    manifest.container,
    "sh",
    "-ceu",
    [
      "python3 -c 'import dbus, dbusmock, gi, packaging'",
      "test -r /source/tests/test_bluez5.py",
      "test -r /source/tests/test_networkmanager.py",
    ].join(" && "),
  ]);
  const elapsed = performance.now() - started;
  if (elapsed > LIFECYCLE_TIMEOUT_MS) {
    throw new Error(`D-Bus startup took ${elapsed}ms`);
  }
  process.stdout.write(
    `D-Bus environment ready in ${Math.round(elapsed)}ms.\n`,
  );
}

function down() {
  const started = performance.now();
  const { manifest } = inputs();
  if (containerExists(manifest.container)) {
    run("docker", ["container", "rm", "--force", manifest.container]);
  }
  const elapsed = performance.now() - started;
  if (elapsed > LIFECYCLE_TIMEOUT_MS) {
    throw new Error(`D-Bus teardown took ${elapsed}ms`);
  }
  process.stdout.write(
    `D-Bus environment stopped in ${Math.round(elapsed)}ms.\n`,
  );
}

const CASES = {
  bluez: [
    "tests.test_bluez5.TestBlueZ5.test_one_adapter",
    "tests.test_bluez5.TestBlueZ5.test_pairing_device",
    "tests.test_bluez5.TestBlueZ5.test_register_advertisement",
    "tests.test_bluez5.TestBlueZ5.test_unregister_advertisement",
    "tests.test_bluez5.TestBlueZ5.test_register_monitor",
    "tests.test_bluez5.TestBlueZ5.test_agent",
  ],
  networkmanager: [
    "tests.test_networkmanager.TestNetworkManager." +
      "test_one_wifi_with_accesspoints",
    "tests.test_networkmanager.TestNetworkManager.test_wifi_with_connection",
    "tests.test_networkmanager.TestNetworkManager.test_global_state",
    "tests.test_networkmanager.TestNetworkManager.test_add_connection",
    "tests.test_networkmanager.TestNetworkManager.test_remove_connection",
  ],
};

function selfTest(kind) {
  try {
    const { manifest } = inputs();
    if (!CASES[kind]) {
      throw new Error(`unknown D-Bus self-test: ${kind}`);
    }
    if (!containerExists(manifest.container)) {
      throw new Error("run dbus-up before the self-test");
    }
    run("docker", [
      "exec",
      "--workdir",
      "/source",
      "--env",
      "PYTHONPATH=/source",
      "--env",
      "LC_ALL=C.UTF-8",
      manifest.container,
      "python3",
      "-m",
      "unittest",
      "--verbose",
      ...CASES[kind],
    ]);
    process.stdout.write(`Private ${kind} D-Bus reference self-test passed.\n`);
  } catch (error) {
    recordFailureArtifact(join(ROOT, "reports", "failures"), {
      events: [{ event: "service", status: "failed" }],
      gate: "private-dbus",
      outcome: { kind: "failed" },
      stage: "service-self-test",
    });
    throw error;
  }
}

function validate() {
  inputs();
  process.stdout.write(
    "Pinned private D-Bus environment configuration passed.\n",
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [, , command] = process.argv;
  const actions = {
    down,
    provision,
    "self-test": () => selfTest(process.argv[3]),
    up,
    validate,
  };
  if (!actions[command]) {
    throw new Error(`unknown D-Bus command: ${command}`);
  }
  actions[command]();
}
