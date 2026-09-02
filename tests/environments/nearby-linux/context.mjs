import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const CONTEXT_NAME = "context";
const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const GLOOP = "gloop";
const NEARBY = "nearby-linux";
const OVERRIDES = [
  "google-ukey2",
  "nlohmann-json",
  "nisaba",
  "protobuf-matchers",
  "sdbus-cpp",
  "smhasher",
];
const SIMPLE_OVERLAYS = new Map([
  ["nlohmann-json", "nlohmann-json.BUILD.bazel"],
  ["smhasher", "smhasher.BUILD.bazel"],
]);
const CLI_ACTIONS_PATCH = "cli-actions.patch";
const SDBUS_BUILD = `load("@rules_foreign_cc//foreign_cc:defs.bzl", "cmake")

filegroup(
    name = "all_srcs",
    srcs = glob(["**"]),
    visibility = ["//visibility:public"],
)

cmake(
    name = "sdbus_cpp",
    lib_source = ":all_srcs",
    out_static_libs = ["libsdbus-c++.a"],
    cache_entries = {
        "BUILD_SHARED_LIBS": "OFF",
        "CMAKE_C_STANDARD": "17",
        "CMAKE_C_STANDARD_REQUIRED": "ON",
        "CMAKE_C_EXTENSIONS": "ON",
        "CMAKE_INSTALL_LIBDIR": "lib",
        "CMAKE_POSITION_INDEPENDENT_CODE": "ON",
        "SDBUSCPP_BUILD_CODEGEN": "OFF",
        "SDBUSCPP_BUILD_DOCS": "OFF",
        "SDBUSCPP_BUILD_EXAMPLES": "OFF",
        "SDBUSCPP_BUILD_TESTS": "OFF",
        "SDBUSCPP_SDBUS_LIB": "systemd",
    },
    deps = ["@libsystemd//:libsystemd"],
    env = {
        "PKG_CONFIG_PATH": "/usr/lib/x86_64-linux-gnu/pkgconfig:" +
            "/usr/lib64/pkgconfig:/usr/share/pkgconfig",
    },
    visibility = ["//visibility:public"],
)
`;

function hash(value) {
  return createHash("sha256").update(value).digest("hex");
}

function assertCachePath(cacheRoot, path) {
  const root = `${resolve(cacheRoot)}${"/"}`;
  if (!`${resolve(path)}${"/"}`.startsWith(root)) {
    throw new Error(`refusing unsafe Nearby Linux cache path: ${path}`);
  }
}

function sourcePath(sourceRoot, source) {
  const path = join(sourceRoot, source);
  if (!existsSync(path)) {
    throw new Error(`missing pinned source ${source}; run make sources-fetch`);
  }
  return path;
}

function copyOverride(sourceRoot, overrides, source) {
  const destination = join(overrides, source);
  cpSync(sourcePath(sourceRoot, source), destination, {
    preserveTimestamps: true,
    recursive: true,
  });
  writeFileSync(join(destination, "WORKSPACE"), "");
  return destination;
}

function prepareSimpleOverlays(overrides, overlayRoot) {
  for (const [source, overlay] of SIMPLE_OVERLAYS) {
    copyFileSync(
      join(overlayRoot, overlay),
      join(overrides, source, "BUILD.bazel"),
    );
  }
}

function prepareNisaba(overrides, overlayRoot) {
  const nisaba = join(overrides, "nisaba", "nisaba", "port");
  copyFileSync(
    join(overlayRoot, "nisaba-port.BUILD.bazel"),
    join(nisaba, "BUILD.bazel"),
  );
  copyFileSync(
    join(overlayRoot, "nisaba-thread-pool.h"),
    join(nisaba, "thread_pool.h"),
  );
}

function prepareOverrides(sourceRoot, context, overlayRoot) {
  const overrides = join(context, "overrides");
  for (const source of OVERRIDES) {
    copyOverride(sourceRoot, overrides, source);
  }
  prepareSimpleOverlays(overrides, overlayRoot);
  prepareNisaba(overrides, overlayRoot);
  writeFileSync(join(overrides, "sdbus-cpp", "BUILD.bazel"), SDBUS_BUILD);
}

function copyRequired(path, destination) {
  if (!existsSync(path)) {
    throw new Error(`required verified input is missing: ${path}`);
  }
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(path, destination);
}

function prepareCache(context, bazel, llvmKey) {
  const cache = join(context, "cache");
  mkdirSync(join(cache, "repository"), { recursive: true });
  copyRequired(bazel, join(cache, "bazel-9.0.1-linux-x86_64"));
  copyRequired(llvmKey, join(cache, "llvm-apt-signing-key.asc"));
}

function applyCliActions(nearby) {
  const patch = join(DIRECTORY, CLI_ACTIONS_PATCH);
  if (!existsSync(patch)) {
    throw new Error("Nearby Linux CLI actions patch is missing");
  }
  const temporary = join(nearby, ".patch-tmp");
  mkdirSync(temporary, { recursive: true });
  const result = spawnSync(
    "patch",
    ["--batch", "--strip=1", "--input", patch],
    {
      cwd: nearby,
      encoding: "utf8",
      env: { ...process.env, TMPDIR: temporary },
    },
  );
  rmSync(temporary, { force: true, recursive: true });
  if (result.status !== 0) {
    throw new Error(`Nearby Linux CLI patch failed: ${result.stderr}`);
  }
}

function prepareNearby(sourceRoot, context) {
  const nearby = join(context, "nearby");
  cpSync(sourcePath(sourceRoot, NEARBY), nearby, {
    preserveTimestamps: true,
    recursive: true,
  });
  cpSync(sourcePath(sourceRoot, GLOOP), join(nearby, "third_party", "gloop"), {
    preserveTimestamps: true,
    recursive: true,
  });
  applyCliActions(nearby);
}

function copyAssets(environment, context) {
  cpSync(join(environment, "assets"), join(context, "assets"), {
    preserveTimestamps: true,
    recursive: true,
  });
  copyFileSync(join(environment, "Dockerfile"), join(context, "Dockerfile"));
}

export function contextFingerprint(inputs) {
  const values = [
    inputs.assets,
    inputs.compose,
    inputs.contextSource,
    inputs.dockerfile,
    inputs.manifestSource,
    inputs.overlays,
    inputs.patch,
    inputs.sources,
  ];
  if (values.some((value) => typeof value !== "string")) {
    throw new Error("Nearby Linux fingerprint inputs must be complete");
  }
  return hash(values.join("\0"));
}

function treeEntries(directory, prefix = "") {
  const entries = [];
  const children = readdirSync(directory, { withFileTypes: true }).sort(
    (left, right) => left.name.localeCompare(right.name),
  );
  for (const child of children) {
    const relativePath = join(prefix, child.name);
    const absolutePath = join(directory, child.name);
    if (child.isDirectory()) {
      entries.push(...treeEntries(absolutePath, relativePath));
    } else if (child.isFile()) {
      entries.push(relativePath, readFileSync(absolutePath));
    }
  }
  return entries;
}

export function treeFingerprint(directory) {
  const digest = createHash("sha256");
  for (const entry of treeEntries(directory)) {
    digest.update(entry).update("\0");
  }
  return digest.digest("hex");
}

export function contextPath(cacheRoot, fingerprint) {
  return join(cacheRoot, "nearby-linux", CONTEXT_NAME, fingerprint);
}

export function prepareContext(options) {
  const destination = contextPath(options.cacheRoot, options.fingerprint);
  assertCachePath(options.cacheRoot, destination);
  const temporary = `${destination}.temporary`;
  assertCachePath(options.cacheRoot, temporary);
  rmSync(temporary, { force: true, recursive: true });
  mkdirSync(temporary, { recursive: true });
  try {
    prepareNearby(options.sourceRoot, temporary);
    prepareOverrides(options.sourceRoot, temporary, options.overlayRoot);
    prepareCache(temporary, options.bazel, options.llvmKey);
    copyAssets(options.environment, temporary);
    rmSync(destination, { force: true, recursive: true });
    renameSync(temporary, destination);
  } catch (error) {
    rmSync(temporary, { force: true, recursive: true });
    throw error;
  }
  return destination;
}

export function contextRelativePath(context, path) {
  const result = relative(context, path);
  if (result.startsWith("..") || result === "") {
    throw new Error(`expected context child path: ${path}`);
  }
  return result;
}
