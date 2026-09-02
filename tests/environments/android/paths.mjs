import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");

export function environmentPaths(cacheRoot = process.env.TEST_ENV_CACHE) {
  const testCache = resolve(cacheRoot ?? join(ROOT, ".cache", "test-env"));
  const root = join(testCache, "android");
  const tools = join(root, "tools");
  const adbHome = join(root, "adb-home");
  return {
    adbHome,
    archives: join(root, "archives"),
    avdHome: join(root, "avds"),
    diagnostics: join(root, "diagnostics"),
    emulatorHome: join(adbHome, ".android"),
    gradleHome: join(root, "gradle-home"),
    probeBuild: join(root, "probe-build"),
    root,
    sdk: join(root, "sdk"),
    state: join(root, "state"),
    tools,
    userHome: join(root, "user"),
  };
}

export function toolPath(paths, record) {
  return join(paths.tools, `${record.id}-${record.revision}`);
}

export function commandPaths(paths, manifest) {
  const commandLine = manifest.packages.find(
    ({ id }) => id === "cmdline-tools;23.0",
  );
  const gradle = manifest.probe.toolchain.find(({ id }) => id === "gradle");
  const java = manifest.probe.toolchain.find(({ id }) => id === "java");
  if (!commandLine || !gradle || !java) {
    throw new Error("Android command tool records are incomplete");
  }
  return {
    adb: join(paths.sdk, "platform-tools", "adb"),
    android: join(
      paths.sdk,
      "cmdline-tools",
      commandLine.revision,
      "bin",
      "android",
    ),
    avdmanager: join(
      paths.sdk,
      "cmdline-tools",
      commandLine.revision,
      "bin",
      "avdmanager",
    ),
    emulator: join(paths.sdk, "emulator", "emulator"),
    gradle: join(
      toolPath(paths, gradle),
      `gradle-${gradle.revision}`,
      "bin",
      "gradle",
    ),
    javaHome: toolPath(paths, java),
  };
}

export function androidEnvironment(paths, commands, manifest) {
  const privateKey = join(paths.emulatorHome, "adbkey");
  return {
    ...process.env,
    ADB_VENDOR_KEYS: privateKey,
    ANDROID_ADB_SERVER_PORT: String(manifest.host.adbServerPort),
    ANDROID_AVD_HOME: paths.avdHome,
    ANDROID_EMULATOR_HOME: paths.emulatorHome,
    ANDROID_HOME: paths.sdk,
    ANDROID_SDK_ROOT: paths.sdk,
    ANDROID_USER_HOME: paths.userHome,
    GRADLE_USER_HOME: paths.gradleHome,
    HOME: paths.adbHome,
    JAVA_HOME: commands.javaHome,
  };
}
