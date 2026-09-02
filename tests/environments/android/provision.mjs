import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../../../tools/gates/lib/process.mjs";
import { ensureExtracted, fetchArchive } from "./archives.mjs";
import {
  androidEnvironment,
  commandPaths,
  environmentPaths,
  toolPath,
} from "./paths.mjs";
import { provisionOrchestrator } from "./orchestrator.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const PROBE_ROOT = join(DIRECTORY, "probe");

const PACKAGE_LOCATIONS = {
  "build-tools;36.1.0": ["build-tools", "36.1.0"],
  emulator: ["emulator"],
  "platform-tools": ["platform-tools"],
  "platforms;android-36": ["platforms", "android-36"],
  "system-images;android-36;google_apis;x86_64": [
    "system-images",
    "android-36",
    "google_apis",
    "x86_64",
  ],
};

export function sdkPackageArgument(record) {
  const parts = record.id.split(";");
  if (parts[0] === "platforms") {
    return `${parts.join("/")}@${record.revision}`;
  }
  if (parts[0] === "system-images") {
    return `${parts.join("/")}@${record.revision}`;
  }
  if (parts[0] === "build-tools") {
    return parts.join("/");
  }
  if (parts.length === 1) {
    return `${record.id}@${record.revision}`;
  }
  throw new Error(`cannot map Android package ${record.id}`);
}

function packageRecords(manifest) {
  return manifest.packages.filter(({ id }) => !id.startsWith("cmdline-tools;"));
}

function commandLineRecord(manifest) {
  const record = manifest.packages.find(({ id }) =>
    id.startsWith("cmdline-tools;"),
  );
  if (!record) {
    throw new Error("Android command-line package is missing");
  }
  return record;
}

function ensureCommandLineSdkLayout(paths, record) {
  const source = join(toolPath(paths, record), "cmdline-tools");
  const parent = join(paths.sdk, "cmdline-tools");
  const destination = join(parent, record.revision);
  const marker = join(destination, ".archive-sha256");
  mkdirSync(parent, { recursive: true });
  if (
    existsSync(marker) &&
    readFileSync(marker, "utf8").trim() === record.sha256
  ) {
    return;
  }
  rmSync(destination, { force: true, recursive: true });
  cpSync(source, destination, { recursive: true });
  writeFileSync(marker, `${record.sha256}\n`);
}

export async function bootstrapAndroid(
  manifest,
  paths = environmentPaths(),
  runCommand = run,
) {
  mkdirSync(paths.tools, { recursive: true });
  const extraction = { manifest, paths, runCommand };
  const commandLine = commandLineRecord(manifest);
  await ensureExtracted({
    ...extraction,
    record: commandLine,
  });
  await Promise.all(
    manifest.probe.toolchain.map((record) =>
      ensureExtracted({ ...extraction, record }),
    ),
  );
  ensureCommandLineSdkLayout(paths, commandLine);
  const commands = commandPaths(paths, manifest);
  const executables = [
    commands.android,
    commands.gradle,
    join(commands.javaHome, "bin", "java"),
  ];
  for (const executable of executables) {
    if (!existsSync(executable)) {
      throw new Error(`Android bootstrap did not create ${executable}`);
    }
  }
  return { commands, paths };
}

export function sdkInstallArguments(paths, records) {
  return [
    `--sdk=${paths.sdk}`,
    "sdk",
    "install",
    ...records.map(sdkPackageArgument),
  ];
}

function assertInteractive() {
  if (!process.stdin.isTTY || !process.stdout.isTTY) {
    throw new Error("Android license review requires an interactive terminal");
  }
}

function licensePath(paths, manifest) {
  return join(paths.sdk, "licenses", manifest.license.identifier);
}

export async function reviewAndroidLicense(manifest) {
  assertInteractive();
  const { commands, paths } = await bootstrapAndroid(manifest);
  const platformTools = manifest.packages.find(
    ({ id }) => id === "platform-tools",
  );
  if (!platformTools) {
    throw new Error("Android platform-tools package is missing");
  }
  mkdirSync(paths.sdk, { recursive: true });
  const environment = androidEnvironment(paths, commands, manifest);
  run(commands.android, sdkInstallArguments(paths, [platformTools]), {
    env: environment,
  });
  if (!existsSync(licensePath(paths, manifest))) {
    throw new Error("Android CLI did not record SDK license acceptance");
  }
}

function properties(path) {
  const values = new Map();
  for (const line of readFileSync(path, "utf8").split("\n")) {
    const separator = line.indexOf("=");
    if (separator > 0) {
      const key = line.slice(0, separator).trim();
      const value = line.slice(separator + 1).trim();
      values.set(key, value);
    }
  }
  return values;
}

function assertInstalledPackage(paths, record) {
  const segments = PACKAGE_LOCATIONS[record.id];
  if (!segments) {
    throw new Error(`Android package location is unknown for ${record.id}`);
  }
  const sourceProperties = join(paths.sdk, ...segments, "source.properties");
  if (!existsSync(sourceProperties)) {
    throw new Error(`Android package ${record.id} was not installed`);
  }
  const installed = properties(sourceProperties).get("Pkg.Revision");
  if (installed !== record.revision) {
    throw new Error(
      `Android package ${record.id} expected ${record.revision}, ` +
        `received ${installed ?? "unknown"}`,
    );
  }
}

async function installSystemImageArchive(manifest, context, record) {
  const segments = PACKAGE_LOCATIONS[record.id];
  const destination = join(context.paths.sdk, ...segments);
  const marker = join(destination, ".archive-sha256");
  if (
    existsSync(marker) &&
    readFileSync(marker, "utf8").trim() === record.sha256
  ) {
    return;
  }
  const staging = join(context.paths.root, "system-image-staging");
  const archive = await fetchArchive({
    manifest,
    paths: context.paths,
    record,
  });
  rmSync(staging, { force: true, recursive: true });
  rmSync(destination, { force: true, recursive: true });
  mkdirSync(staging, { recursive: true });
  try {
    run("unzip", ["-q", archive, "-d", staging]);
    mkdirSync(dirname(destination), { recursive: true });
    renameSync(join(staging, "x86_64"), destination);
    writeFileSync(marker, `${record.sha256}\n`);
  } finally {
    rmSync(staging, { force: true, recursive: true });
  }
}

function createAvd(manifest, context, peer) {
  const avdPath = join(context.paths.avdHome, `${peer.name}.avd`);
  const configPath = join(avdPath, "config.ini");
  const expectedImage = manifest.avds.systemImage.replaceAll(";", "/");
  let configuredImage = null;
  if (existsSync(configPath)) {
    configuredImage = properties(configPath).get("image.sysdir.1");
  }
  if (configuredImage?.includes(expectedImage)) {
    return;
  }
  rmSync(avdPath, { force: true, recursive: true });
  rmSync(join(context.paths.avdHome, `${peer.name}.ini`), { force: true });
  const argumentsList = [
    "create",
    "avd",
    "--force",
    "--name",
    peer.name,
    "--package",
    manifest.avds.systemImage,
    "--device",
    manifest.avds.hardwareProfile,
  ];
  run(context.commands.avdmanager, argumentsList, {
    env: context.environment,
    input: "no\n",
  });
}

function buildProbe(context) {
  mkdirSync(context.paths.probeBuild, { recursive: true });
  const argumentsList = [
    "--no-daemon",
    "--console=plain",
    "--project-cache-dir",
    join(context.paths.root, "gradle-project-cache"),
    `-PquickshareBuildRoot=${context.paths.probeBuild}`,
    ":app:assembleDebug",
    ":app:lintDebug",
  ];
  run(context.commands.gradle, argumentsList, {
    cwd: PROBE_ROOT,
    env: context.environment,
  });
  const apk = join(
    context.paths.probeBuild,
    "app",
    "outputs",
    "apk",
    "debug",
    "app-debug.apk",
  );
  if (!existsSync(apk)) {
    throw new Error("Android probe build did not produce its debug APK");
  }
}

export async function provisionAndroid(manifest) {
  const context = await bootstrapAndroid(manifest);
  provisionOrchestrator(manifest);
  context.environment = androidEnvironment(
    context.paths,
    context.commands,
    manifest,
  );
  if (!existsSync(licensePath(context.paths, manifest))) {
    throw new Error(
      "Android SDK license is missing; run `make android-license`",
    );
  }
  const records = packageRecords(manifest);
  const systemImage = records.find(
    ({ id }) => id === manifest.avds.systemImage,
  );
  if (!systemImage) {
    throw new Error("Android system image package is missing");
  }
  await installSystemImageArchive(manifest, context, systemImage);
  const managedRecords = records.filter(({ id }) => id !== systemImage.id);
  run(
    context.commands.android,
    sdkInstallArguments(context.paths, managedRecords),
    { env: context.environment },
  );
  for (const record of records) {
    assertInstalledPackage(context.paths, record);
  }
  mkdirSync(context.paths.avdHome, { recursive: true });
  for (const peer of manifest.avds.peers) {
    createAvd(manifest, context, peer);
  }
  buildProbe(context);
  process.stdout.write(
    "Pinned Android SDK, AVDs, and probe are provisioned.\n",
  );
}
