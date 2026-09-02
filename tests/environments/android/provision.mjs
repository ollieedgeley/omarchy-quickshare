import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import { run } from "../../../tools/gates/lib/process.mjs";
import { ensureExtracted } from "./archives.mjs";
import {
  androidEnvironment,
  commandPaths,
  environmentPaths,
} from "./paths.mjs";

const PACKAGE_LOCATIONS = {
  "build-tools;36.1.0": ["build-tools", "36.1.0"],
  emulator: ["emulator"],
  "platform-tools": ["platform-tools"],
  "platforms;android-36": ["platforms", "android-36"],
  "system-images;android-36;google_apis_playstore;x86_64": [
    "system-images",
    "android-36",
    "google_apis_playstore",
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

export async function bootstrapAndroid(
  manifest,
  paths = environmentPaths(),
  runCommand = run,
) {
  mkdirSync(paths.tools, { recursive: true });
  const extraction = { manifest, paths, runCommand };
  await ensureExtracted({
    ...extraction,
    record: commandLineRecord(manifest),
  });
  await Promise.all(
    manifest.probe.toolchain.map((record) =>
      ensureExtracted({ ...extraction, record }),
    ),
  );
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
  const environment = androidEnvironment(paths, commands);
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

function createAvd(manifest, context, peer) {
  const avdPath = join(context.paths.avdHome, `${peer.name}.avd`);
  if (existsSync(join(avdPath, "config.ini"))) {
    return;
  }
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

export async function provisionAndroid(manifest) {
  const context = await bootstrapAndroid(manifest);
  context.environment = androidEnvironment(context.paths, context.commands);
  if (!existsSync(licensePath(context.paths, manifest))) {
    throw new Error(
      "Android SDK license is missing; run `make android-license`",
    );
  }
  const records = packageRecords(manifest);
  run(context.commands.android, sdkInstallArguments(context.paths, records), {
    env: context.environment,
  });
  for (const record of records) {
    assertInstalledPackage(context.paths, record);
  }
  mkdirSync(context.paths.avdHome, { recursive: true });
  for (const peer of manifest.avds.peers) {
    createAvd(manifest, context, peer);
  }
  process.stdout.write("Pinned Android SDK and both AVDs are provisioned.\n");
}
