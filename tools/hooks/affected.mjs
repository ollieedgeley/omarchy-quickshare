import { dirname, relative, resolve, sep } from "node:path";

export function parseAffectedJson(value) {
  if (
    !value ||
    typeof value !== "object" ||
    !Array.isArray(value.affectedTests)
  ) {
    return [];
  }
  const paths = value.affectedTests
    .filter((path) => typeof path === "string")
    .map((path) => path.trim())
    .filter((path) => path.length > 0);
  return [...new Set(paths)].sort();
}
export function codeGraphAffectedArgs(repositoryPath, paths) {
  return [
    "affected",
    "--path",
    repositoryPath,
    "--depth",
    "32",
    "--json",
    ...paths,
  ];
}

export function packageSelection(metadata, paths, workspaceRoot) {
  const workspaceIds = new Set(metadata.workspace_members);
  const packages = metadata.packages.filter((pkg) => workspaceIds.has(pkg.id));
  const owners = new Set();
  for (const path of paths) {
    const absolute = resolve(workspaceRoot, path);
    const candidates = packages
      .filter((pkg) => {
        const packageRoot = dirname(pkg.manifest_path);
        return (
          absolute === packageRoot ||
          absolute.startsWith(`${packageRoot}${sep}`)
        );
      })
      .sort(
        (left, right) => right.manifest_path.length - left.manifest_path.length,
      );
    if (candidates[0]) {
      owners.add(candidates[0].id);
    }
  }

  const reverse = new Map();
  for (const node of metadata.resolve?.nodes ?? []) {
    for (const dependency of node.dependencies ?? []) {
      const dependents = reverse.get(dependency) ?? new Set();
      dependents.add(node.id);
      reverse.set(dependency, dependents);
    }
  }
  const selected = new Set(owners);
  const queue = [...owners];
  while (queue.length) {
    const current = queue.shift();
    for (const dependent of reverse.get(current) ?? []) {
      if (workspaceIds.has(dependent) && !selected.has(dependent)) {
        selected.add(dependent);
        queue.push(dependent);
      }
    }
  }

  return packages
    .filter((pkg) => selected.has(pkg.id))
    .map((pkg) => ({
      hasLibrary: pkg.targets.some((target) => target.kind.includes("lib")),
      name: pkg.name,
      root: relative(workspaceRoot, dirname(pkg.manifest_path)),
    }))
    .sort((left, right) => left.name.localeCompare(right.name));
}

const RUST_INPUT_PATTERN = /(?:^|\/)Cargo\.(?:lock|toml)$/u;

export function isRustInput(path) {
  return (
    path.endsWith(".rs") ||
    RUST_INPUT_PATTERN.test(path) ||
    ["clippy.toml", "rust-toolchain.toml", "rustfmt.toml"].includes(path)
  );
}

function workspacePackages(metadata, workspaceRoot) {
  const workspaceIds = new Set(metadata.workspace_members);
  return metadata.packages
    .filter((pkg) => workspaceIds.has(pkg.id))
    .map((pkg) => ({
      hasLibrary: pkg.targets.some((target) => target.kind.includes("lib")),
      name: pkg.name,
      root: relative(workspaceRoot, dirname(pkg.manifest_path)),
    }))
    .sort((left, right) => left.name.localeCompare(right.name));
}

export function selectRustPackages(metadata, paths, workspaceRoot) {
  const selected = packageSelection(metadata, paths, workspaceRoot);
  if (selected.length || !paths.some(isRustInput)) {
    return selected;
  }
  return workspacePackages(metadata, workspaceRoot);
}
function getExt(path) {
  const name = path.split("/").pop();
  const dot = name.lastIndexOf(".");
  if (dot === -1) {
    return "";
  }
  return name.slice(dot + 1).toLowerCase();
}

export function isCodeGraphIndexableSource(path) {
  if (typeof path !== "string" || path.length === 0) {
    return false;
  }
  const extension = getExt(path);
  const allowed = [
    "rs",
    "js",
    "mjs",
    "cjs",
    "jsx",
    "ts",
    "tsx",
    "py",
    "pyi",
    "sh",
    "bash",
    "c",
    "cc",
    "cpp",
    "cxx",
    "h",
    "hh",
    "hpp",
    "hxx",
    "java",
    "kt",
    "kts",
  ];
  if (!allowed.includes(extension)) {
    return false;
  }
  const excludedDirectories =
    /(?:^|\/)(?:node_modules|\.cache|target)(?:\/|$)/u;
  if (excludedDirectories.test(path)) {
    return false;
  }
  if (path.startsWith(".") && !path.startsWith(".omp/")) {
    return false;
  }
  return true;
}
export function isTestPath(path) {
  if (!isCodeGraphIndexableSource(path)) {
    return false;
  }
  const name = path.split("/").pop();
  const testMarker = /\.(?:test|spec)\./u;
  const testSuffix = /_test\./u;
  const testPrefix = /^test_/u;
  const testName =
    testMarker.test(name) ||
    testSuffix.test(name) ||
    testPrefix.test(name) ||
    name === "tests.rs";
  const testDirectory = /\/tests?\//u;
  const inTestDirectory =
    testDirectory.test(path) ||
    path.startsWith("tests/") ||
    path.startsWith("test/");
  return testName || inTestDirectory;
}

export function getLanguage(path) {
  if (typeof path !== "string") {
    return "unknown";
  }
  const extension = getExt(path);
  if (extension === "rs") {
    return "rust";
  }
  if (["js", "mjs", "cjs", "jsx", "ts", "tsx"].includes(extension)) {
    return "javascript";
  }
  if (["py", "pyi"].includes(extension)) {
    return "python";
  }
  if (["sh", "bash"].includes(extension)) {
    return "shell";
  }
  if (["c", "cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"].includes(extension)) {
    return "cpp";
  }
  if (extension === "java") {
    return "java";
  }
  if (["kt", "kts"].includes(extension)) {
    return "kotlin";
  }
  if (extension === "qml") {
    return "qml";
  }
  return "unknown";
}

function isWithin(path, root) {
  return path === root || path.startsWith(`${root}/`);
}

function matchEnvDomain(path) {
  if (!path.startsWith("tests/environments/")) {
    return null;
  }
  const ENV_RE = /tests\/environments\/(?<env>[^/]+)/u;
  const match = path.match(ENV_RE);
  if (match) {
    return `tests/environments/${match.groups.env}`;
  }
  return null;
}

function matchSuiteDomain(path) {
  if (!path.startsWith("tests/suites/")) {
    return null;
  }
  const SUITE_RE = /tests\/suites\/(?<suite>[^/]+)/u;
  const match = path.match(SUITE_RE);
  if (match) {
    return `tests/suites/${match.groups.suite}`;
  }
  return null;
}

function matchCoreCrateDomain(path) {
  if (!path.startsWith("crates/core/")) {
    return null;
  }
  const CORE_RE = /crates\/core\/(?<crate>[^/]+)/u;
  const match = path.match(CORE_RE);
  if (match) {
    return `crates/core/${match.groups.crate}`;
  }
  return null;
}

function matchPlatformCrateDomain(path) {
  if (!path.startsWith("crates/platform/")) {
    return null;
  }
  const PLATFORM_RE = /crates\/platform\/(?<crate>[^/]+)/u;
  const match = path.match(PLATFORM_RE);
  if (match) {
    return `crates/platform/${match.groups.crate}`;
  }
  return null;
}

function matchFixedDomain(path) {
  const fixedDomains = [
    {
      domain: "plugin-release",
      roots: ["tools/release", "packaging/omarchy-plugin"],
    },
    { domain: "oracle", roots: ["tools/oracle"] },
    { domain: "crates/app", roots: ["crates/app"] },
    {
      domain: "tooling",
      roots: [
        ".omp",
        "tools/hooks",
        "tools/gates",
        "tools/setup",
        "rules/ast-grep",
      ],
    },
  ];
  return fixedDomains.find(({ roots }) =>
    roots.some((root) => isWithin(path, root)),
  )?.domain;
}

function matchRootDomain(path) {
  if (path.includes("/")) {
    return null;
  }
  const cargoFiles = new Set([
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "clippy.toml",
  ]);
  if (cargoFiles.has(path)) {
    return "cargo-workspace";
  }
  const toolingFiles = new Set([
    "Makefile",
    "package.json",
    "package-lock.json",
    "eslint.config.mjs",
    ".prettierrc.json",
    "ruff.toml",
    "sgconfig.yml",
    "biome.json",
  ]);
  if (toolingFiles.has(path)) {
    return "tooling";
  }
  if (path.endsWith(".md")) {
    return "docs";
  }
  return null;
}

export function getRepositoryDomain(path) {
  if (typeof path !== "string") {
    return "other";
  }
  const domain =
    matchEnvDomain(path) ??
    matchSuiteDomain(path) ??
    matchFixedDomain(path) ??
    matchCoreCrateDomain(path) ??
    matchPlatformCrateDomain(path) ??
    matchRootDomain(path);
  return domain ?? "other";
}

function toRecord(path) {
  return {
    path,
    language: getLanguage(path),
    domain: getRepositoryDomain(path),
  };
}

export function computeSelectionRecord(stagedPaths, affectedTests = []) {
  const staged = [
    ...new Set(stagedPaths.filter((path) => typeof path === "string")),
  ].sort();
  const stagedTestPaths = staged.filter(isTestPath);
  const stagedNonTestPaths = staged.filter((path) => !isTestPath(path));
  const stagedTests = stagedTestPaths.map(toRecord);
  const stagedSourcesRec = stagedNonTestPaths.map(toRecord);
  const affectedTestPaths = affectedTests.filter(
    (test) => typeof test === "string" && isTestPath(test),
  );
  const extendedTestPaths = [
    ...new Set(
      affectedTestPaths.filter((test) => !stagedTestPaths.includes(test)),
    ),
  ].sort();
  const extendedTests = extendedTestPaths.map(toRecord);
  return { stagedSources: stagedSourcesRec, stagedTests, extendedTests };
}
