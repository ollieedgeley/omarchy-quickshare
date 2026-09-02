import { accessSync, constants, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../tools/gates/lib/process.mjs";
import { orchestratorFingerprint } from "./orchestrator.mjs";
import { environmentPaths } from "./paths.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const RUNNER = join(DIRECTORY, "probe", "runner");
const CONFIGURATION = join(RUNNER, "mobly-config.yml");
const TEST = join(RUNNER, "nearby_connections_test.py");
const FINGERPRINT_LABEL = "io.omarchy-quickshare.environment";
const DEFAULT_USER_ID = 1000;
const CONTAINER_PATH = [
  "/android-platform-tools",
  "/usr/local/bin",
  "/usr/local/sbin",
  "/usr/bin",
  "/usr/sbin",
  "/bin",
  "/sbin",
].join(":");

function docker() {
  return process.env.DOCKER ?? "docker";
}

function assertImage(manifest) {
  const { image } = manifest.probe.orchestrator;
  const label = output(docker(), [
    "image",
    "inspect",
    "--format",
    `{{index .Config.Labels "${FINGERPRINT_LABEL}"}}`,
    image,
  ]);
  if (label !== orchestratorFingerprint(manifest)) {
    throw new Error("Mobly image inputs changed; run `make android-provision`");
  }
}

export function orchestratorRunArguments(manifest, paths, ids) {
  const logs = join(paths.diagnostics, "mobly");
  return [
    "run",
    "--rm",
    "--network=host",
    "--read-only",
    "--security-opt=no-new-privileges",
    "--cap-drop=ALL",
    "--user",
    `${ids.uid}:${ids.gid}`,
    "--env",
    "HOME=/tmp",
    "--env",
    `ANDROID_ADB_SERVER_PORT=${manifest.host.adbServerPort}`,
    "--env",
    `PATH=${CONTAINER_PATH}`,
    "--tmpfs",
    "/tmp:rw,noexec,nosuid,size=64m",
    "--volume",
    `${join(paths.sdk, "platform-tools")}:/android-platform-tools:ro`,
    "--volume",
    `${RUNNER}:/work:ro`,
    "--volume",
    `${logs}:/logs`,
    "--workdir",
    "/work",
    manifest.probe.orchestrator.image,
    "nearby_connections_test.py",
    "--config",
    "mobly-config.yml",
  ];
}

export function selfTest(manifest) {
  const paths = environmentPaths();
  accessSync(CONFIGURATION, constants.R_OK);
  accessSync(TEST, constants.R_OK);
  accessSync(join(paths.sdk, "platform-tools", "adb"), constants.X_OK);
  assertImage(manifest);
  mkdirSync(join(paths.diagnostics, "mobly"), { recursive: true });
  const ids = {
    gid: process.getgid?.() ?? DEFAULT_USER_ID,
    uid: process.getuid?.() ?? DEFAULT_USER_ID,
  };
  run(docker(), orchestratorRunArguments(manifest, paths, ids));
}
