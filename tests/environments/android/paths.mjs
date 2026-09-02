import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");

export function environmentPaths(cacheRoot = process.env.TEST_ENV_CACHE) {
  const testCache = resolve(cacheRoot ?? join(ROOT, ".cache", "test-env"));
  const root = join(testCache, "android");
  const tools = join(root, "tools");
  return {
    archives: join(root, "archives"),
    avdHome: join(root, "avds"),
    root,
    sdk: join(root, "sdk"),
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
    android: join(
      toolPath(paths, commandLine),
      "cmdline-tools",
      "bin",
      "android",
    ),
    avdmanager: join(
      toolPath(paths, commandLine),
      "cmdline-tools",
      "bin",
      "avdmanager",
    ),
    gradle: join(
      toolPath(paths, gradle),
      `gradle-${gradle.revision}`,
      "bin",
      "gradle",
    ),
    javaHome: toolPath(paths, java),
  };
}

export function androidEnvironment(paths, commands) {
  return {
    ...process.env,
    ANDROID_AVD_HOME: paths.avdHome,
    ANDROID_HOME: paths.sdk,
    ANDROID_SDK_ROOT: paths.sdk,
    ANDROID_USER_HOME: paths.userHome,
    JAVA_HOME: commands.javaHome,
  };
}
