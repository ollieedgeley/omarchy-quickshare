import { existsSync, mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve } from "node:path";
import { pathToFileURL } from "node:url";

import {
  getRepositoryDomain,
  isRustInput,
  isTestPath,
} from "../../hooks/affected.mjs";
import { run } from "./process.mjs";

const JAVASCRIPT_EXT = /\.[cm]?[jt]sx?$/u;
const KNIP_CONFIG = /(?:^|\/)(?:knip|package(?:-lock)?)\.json$/u;
const PYTHON_EXT = /\.pyi?$/u;
const CPP_EXT = /\.(?:c|cc|cpp|cxx|h|hh|hpp|hxx)$/u;
const CPP_SOURCE_EXT = /\.(?:c|cc|cpp|cxx)$/u;
const CLANG_TIDY_ROOT = "tools/oracle/connections-peer/";
const EXCLUDED_DIRECTORIES = new Set([
  ".cache",
  ".codegraph",
  ".git",
  ".ruff_cache",
  "build",
  "dist",
  "node_modules",
  "reports",
  "target",
]);
const VULTURE_VERSION = "2.16";
const CARGO_MACHETE_VERSION = "0.9.2";
const RUFF_VERSION = "0.16.5";
const REQUIRED_PROTO_PATHS = [
  "connections/implementation/proto/offline_wire_formats.proto",
  "internal/proto/credential.proto",
  "internal/proto/local_credential.proto",
  "internal/proto/metadata.proto",
  "proto/connections_enums.proto",
  "proto/errorcode/error_code_enums.proto",
  "proto/mediums/ble_frames.proto",
  "proto/mediums/multiplex_frames.proto",
  "proto/mediums/nfc_frames.proto",
  "proto/mediums/web_rtc_signaling_frames.proto",
  "proto/sharing_enums.proto",
  "sharing/proto/wire_format.proto",
];

function unique(paths) {
  return [...new Set(paths)].sort();
}

export function duplicationScanPaths(paths, scope = "files") {
  if (scope === "full") {
    return ["."];
  }
  if (paths.length > 0 && paths.every((path) => isTestPath(path))) {
    return [];
  }
  return paths;
}

const FULL_ANALYZER_MODES = new Map([
  ["--full-general", ["jscpd", "cargo-machete", "knip", "ruff", "vulture"]],
  ["--full-clang-tidy", ["clang-tidy"]],
  ["--full-cppcheck", ["cppcheck"]],
]);

export function analyzersForPaths(paths, requestedAnalyzers) {
  if (!paths.length) {
    return [];
  }
  const tools = ["jscpd"];
  if (paths.some(isRustInput)) {
    tools.push("cargo-machete");
  }
  if (
    paths.some((path) => JAVASCRIPT_EXT.test(path) || KNIP_CONFIG.test(path))
  ) {
    tools.push("knip");
  }
  if (paths.some((path) => PYTHON_EXT.test(path))) {
    tools.push("ruff", "vulture");
  }
  if (paths.some((path) => CPP_EXT.test(path))) {
    tools.push("clang-tidy", "cppcheck");
  }
  if (!requestedAnalyzers) {
    return tools;
  }
  const requested = new Set(requestedAnalyzers);
  return tools.filter((tool) => requested.has(tool));
}

export function selectDomainPaths(stagedPaths, repositoryPaths) {
  const domains = new Set(stagedPaths.map(getRepositoryDomain));
  return unique(
    repositoryPaths.filter((path) => domains.has(getRepositoryDomain(path))),
  );
}

export function listProjectFiles(root) {
  const paths = [];
  function walk(directory) {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const absolute = join(directory, entry.name);
      const repositoryPath = relative(root, absolute);
      const generatedHusky =
        repositoryPath === ".husky/_" || repositoryPath.startsWith(".husky/_/");
      const excluded =
        generatedHusky ||
        (entry.isDirectory() && EXCLUDED_DIRECTORIES.has(entry.name));
      if (!excluded) {
        if (entry.isDirectory()) {
          walk(absolute);
        } else if (entry.isFile()) {
          paths.push(repositoryPath);
        }
      }
    }
  }
  walk(root);
  return unique(paths);
}

function executable(name, options) {
  const { nodeBin, toolRoot } = options;
  const overrides = {
    "cargo-machete": process.env.CARGO_MACHETE,
    "clang-tidy": process.env.CLANG_TIDY,
    cppcheck: process.env.CPPCHECK,
    jscpd: process.env.JSCPD,
    knip: process.env.KNIP,
    ruff: process.env.RUFF,
    vulture: process.env.VULTURE,
  };
  if (overrides[name]) {
    return overrides[name];
  }
  if (name === "cargo-machete") {
    return join(
      toolRoot,
      ".cache",
      "tools",
      `cargo-machete-${CARGO_MACHETE_VERSION}`,
      "bin",
      "cargo-machete",
    );
  }
  if (name === "ruff") {
    return join(toolRoot, ".cache", "tools", `ruff-${RUFF_VERSION}`, "ruff");
  }
  if (name === "vulture") {
    return join(
      toolRoot,
      ".cache",
      "tools",
      `vulture-${VULTURE_VERSION}`,
      "bin",
      "vulture",
    );
  }
  if (["jscpd", "knip"].includes(name)) {
    return join(nodeBin, name);
  }
  return name;
}

function protoFiles(sourceRoot) {
  return REQUIRED_PROTO_PATHS.map((path) => join(sourceRoot, path));
}

function prepareNativeIncludes(cwd, sourceRoot) {
  if (!existsSync(sourceRoot)) {
    throw new Error(
      `Nearby source cache is missing at ${sourceRoot}; run make sources-fetch`,
    );
  }
  const overlay = mkdtempSync(join(tmpdir(), "quickshare-native-analysis-"));
  run(
    "protoc",
    [
      `--proto_path=${sourceRoot}`,
      "--proto_path=/usr/include",
      `--cpp_out=${overlay}`,
      ...protoFiles(sourceRoot),
    ],
    { cwd },
  );
  return overlay;
}

function cppcheckArguments(options) {
  const { fixtureInclude, nativePaths, peerInclude, scope } = options;
  const args = [
    "--enable=all",
    "--check-level=exhaustive",
    "--inconclusive",
    "--error-exitcode=1",
    "--std=c++20",
    "--platform=unix64",
    "--language=c++",
    "--suppress=missingInclude",
    "--suppress=missingIncludeSystem",
  ];
  if (scope === "files") {
    args.push(
      "--suppress=unmatchedSuppression",
      "--suppress=unusedFunction",
      "--suppress=unusedStructMember",
    );
  }
  args.push(`-I${fixtureInclude}`, `-I${peerInclude}`, ...nativePaths);
  return args;
}

function runClangTidy(translationUnits, options) {
  if (!translationUnits.length) {
    return;
  }
  const { cwd, fixtureInclude, peerInclude, sourceRoot, tools } = options;
  const overlay = prepareNativeIncludes(cwd, sourceRoot);
  const dependencyInclude = join(sourceRoot, "..", "nlohmann-json", "include");
  const compilerArgs = [
    "-std=c++20",
    `-I${fixtureInclude}`,
    `-I${peerInclude}`,
    `-I${overlay}`,
    `-isystem${dependencyInclude}`,
    `-isystem${sourceRoot}`,
    `-isystem${join(sourceRoot, "compiled_proto")}`,
  ];
  try {
    for (const path of translationUnits) {
      run(
        tools["clang-tidy"],
        [
          path,
          `--config-file=${join(cwd, ".clang-tidy")}`,
          "--",
          ...compilerArgs,
        ],
        { cwd },
      );
    }
  } finally {
    rmSync(overlay, { force: true, recursive: true });
  }
}

function runCppcheck(nativePaths, options) {
  const { cwd, fixtureInclude, peerInclude, scope, tools } = options;
  if (scope === "files") {
    const args = cppcheckArguments({
      fixtureInclude,
      nativePaths,
      peerInclude,
      scope,
    });
    run(tools.cppcheck, args, { cwd });
    return;
  }
  const groups = [
    {
      paths: nativePaths.filter((path) => CPP_SOURCE_EXT.test(path)),
      scope,
    },
    {
      paths: nativePaths.filter((path) => !CPP_SOURCE_EXT.test(path)),
      scope: "files",
    },
  ];
  for (const group of groups) {
    if (group.paths.length) {
      const args = cppcheckArguments({
        fixtureInclude,
        nativePaths: group.paths,
        peerInclude,
        scope: group.scope,
      });
      run(tools.cppcheck, args, { cwd });
    }
  }
}

function runNative(paths, options) {
  const { cwd, scope, sourceRoot, tools } = options;
  const nativePaths = paths.filter((path) => CPP_EXT.test(path));
  const translationUnits = nativePaths.filter(
    (path) => CPP_SOURCE_EXT.test(path) && path.startsWith(CLANG_TIDY_ROOT),
  );
  if (!nativePaths.length) {
    return;
  }
  const fixtureInclude = join(cwd, "tools", "oracle", "sharing-fixtures");
  const peerInclude = join(cwd, "tools", "oracle", "connections-peer");

  if (tools["clang-tidy"]) {
    runClangTidy(translationUnits, {
      cwd,
      fixtureInclude,
      peerInclude,
      sourceRoot,
      tools,
    });
  }
  if (tools.cppcheck) {
    runCppcheck(nativePaths, {
      cwd,
      fixtureInclude,
      peerInclude,
      scope,
      tools,
    });
  }
}

function runCargoMachete(tools, cargoPackages, cwd) {
  const roots = unique(cargoPackages.map((pkg) => pkg.root));
  if (!roots.length) {
    roots.push(".");
  }
  run(
    tools["cargo-machete"],
    ["--with-metadata", "--skip-target-dir", ...roots],
    { cwd },
  );
}

function runPython(tools, paths, cwd) {
  const pythonPaths = paths.filter((path) => PYTHON_EXT.test(path));
  if (!pythonPaths.length) {
    return;
  }
  if (tools.ruff) {
    run(tools.ruff, ["check", ...pythonPaths], { cwd });
  }
  if (tools.vulture) {
    run(tools.vulture, ["--min-confidence", "100", ...pythonPaths], { cwd });
  }
}
function runDuplication(tools, paths, options) {
  const duplicationPaths = duplicationScanPaths(paths, options.scope);
  if (tools.jscpd && duplicationPaths.length) {
    run(tools.jscpd, ["--config", ".jscpd.json", ...duplicationPaths], {
      cwd: options.cwd,
    });
  }
}

export function runAnalysis(options) {
  const {
    cargoPackages = [],
    cwd = process.cwd(),
    paths,
    requestedAnalyzers,
    scope = "files",
    toolRoot = cwd,
  } = options;
  const existingPaths = unique(
    paths.filter((path) => existsSync(resolve(cwd, path))),
  );
  const analyzers = analyzersForPaths(existingPaths, requestedAnalyzers);
  if (!analyzers.length) {
    return;
  }
  const defaultNodeBin = join(toolRoot, "node_modules", ".bin");
  const nodeBin = process.env.NODE_BIN ?? defaultNodeBin;
  const tools = Object.fromEntries(
    analyzers.map((name) => [name, executable(name, { nodeBin, toolRoot })]),
  );
  runDuplication(tools, existingPaths, { cwd, scope });
  if (analyzers.includes("cargo-machete")) {
    runCargoMachete(tools, cargoPackages, cwd);
  }
  if (analyzers.includes("knip")) {
    run(tools.knip, ["--strict", "--reporter", "compact"], {
      cwd,
    });
  }
  runPython(tools, existingPaths, cwd);
  if (analyzers.includes("clang-tidy") || analyzers.includes("cppcheck")) {
    const testEnvironmentCache =
      process.env.TEST_ENV_CACHE ?? join(toolRoot, ".cache", "test-env");
    runNative(existingPaths, {
      cwd,
      scope,
      sourceRoot: join(
        testEnvironmentCache,
        "sources",
        "trees",
        "nearby-linux",
      ),
      tools,
    });
  }
}

const isDirectExecution =
  process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
if (isDirectExecution) {
  const cwd = process.cwd();
  const [mode] = process.argv.slice(2);
  const requestedAnalyzers = FULL_ANALYZER_MODES.get(mode);
  if (!requestedAnalyzers) {
    throw new Error(
      "usage: analysis.mjs --full-general|--full-clang-tidy|--full-cppcheck",
    );
  }
  runAnalysis({
    cwd,
    paths: listProjectFiles(cwd),
    requestedAnalyzers,
    scope: "full",
  });
}
