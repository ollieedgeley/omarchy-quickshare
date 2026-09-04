import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { join, relative } from "node:path";

import { output, run } from "../gates/lib/process.mjs";
import {
  listProjectFiles,
  runAnalysis,
  selectDomainPaths,
} from "../gates/lib/analysis.mjs";
import { structureScope } from "../gates/structure.mjs";
import {
  codeGraphAffectedArgs,
  computeSelectionRecord,
  isCodeGraphIndexableSource,
  isRustInput,
  parseAffectedJson,
  selectRustPackages,
} from "./affected.mjs";

const ROOT = output("git", ["rev-parse", "--show-toplevel"]);
const metadataPath = join(ROOT, ".cache", "gates", "staged.json");
if (!existsSync(metadataPath)) {
  throw new Error("staged metadata is missing; run `make pre-commit-prepare`");
}
const staged = JSON.parse(readFileSync(metadataPath, "utf8"));
const currentTree = output("git", ["write-tree"], { cwd: ROOT });
if (currentTree !== staged.tree) {
  throw new Error("the Git index changed; rerun `make pre-commit-prepare`");
}

const [, , mode] = process.argv;
const paths = staged.changes
  .flatMap((change) => [change.path, change.oldPath])
  .filter(Boolean);
const existing = [
  ...new Set(paths.filter((path) => existsSync(join(staged.mirror, path)))),
];

const direct = computeSelectionRecord(existing, []);
const sourcePaths = direct.stagedSources.map((record) => record.path);
const testPaths = direct.stagedTests.map((record) => record.path);

const nodeBin = process.env.NODE_BIN ?? join(ROOT, "node_modules", ".bin");
const tool = (name) => join(nodeBin, name);
const codegraph = process.env.CODEGRAPH ?? tool("codegraph");
const ruff =
  process.env.RUFF ?? join(ROOT, ".cache", "tools", "ruff-0.16.5", "ruff");
const astGrep = process.env.AST_GREP ?? tool("ast-grep");
const JS_SOURCE_EXT = /\.[cm]?[jt]sx?$/u;
const PRETTIER_SUPPORTED_EXT = /\.(?:[cm]?[jt]sx?|jsonc?|markdown|md|ya?ml)$/u;
const PYTHON_EXT = /\.pyi?$/u;
const TEST_FILE_EXT = /\.(?:test|spec)\.(?:js|mjs|cjs)$/u;
const RUST_FILE_EXT = /\.rs$/u;

function runStructure() {
  const scopes = new Set(paths.map(structureScope));
  for (const scope of scopes) {
    if (scope) {
      run("node", ["tools/gates/structure.mjs", scope], {
        cwd: staged.mirror,
        env: { ...process.env, AST_GREP: astGrep, GATE_ROOT: staged.mirror },
      });
    }
  }
}

function runFormat(phasePaths) {
  const rustPhase = phasePaths.filter((phasePath) => phasePath.endsWith(".rs"));
  if (rustPhase.length) {
    run("rustfmt", ["--edition", "2024", "--check", ...rustPhase], {
      cwd: staged.mirror,
    });
  }
  const prettierSupported = phasePaths.filter((phasePath) =>
    PRETTIER_SUPPORTED_EXT.test(phasePath),
  );
  if (prettierSupported.length) {
    run(tool("prettier"), ["--check", ...prettierSupported], {
      cwd: staged.mirror,
    });
  }
  const pyPhase = phasePaths.filter((phasePath) => PYTHON_EXT.test(phasePath));
  if (pyPhase.length) {
    run(ruff, ["format", "--check", ...pyPhase], {
      cwd: staged.mirror,
    });
  }
}

function cargoMetadata() {
  const result = run(
    "cargo",
    ["metadata", "--format-version", "1", "--locked"],
    {
      cwd: staged.mirror,
      capture: true,
      quiet: true,
    },
  );
  return JSON.parse(result.stdout);
}
function runLint(phasePaths, phase) {
  const jsPhase = phasePaths.filter((phasePath) =>
    JS_SOURCE_EXT.test(phasePath),
  );
  if (jsPhase.length) {
    run(
      tool("eslint"),
      ["--max-warnings", "0", "--no-warn-ignored", ...jsPhase],
      { cwd: staged.mirror },
    );
  }
  const pyPhase = phasePaths.filter((phasePath) => PYTHON_EXT.test(phasePath));
  if (pyPhase.length) {
    run(ruff, ["check", ...pyPhase], { cwd: staged.mirror });
  }
  if (!phasePaths.some(isRustInput)) {
    return;
  }
  const cargo = cargoMetadata();
  let packages = selectRustPackages(cargo, phasePaths, staged.mirror);
  if (phase === "test" && sourcePaths.some(isRustInput)) {
    const sourcePackages = selectRustPackages(
      cargo,
      sourcePaths,
      staged.mirror,
    );
    const sourceNames = new Set(sourcePackages.map((pkg) => pkg.name));
    packages = packages.filter((pkg) => !sourceNames.has(pkg.name));
  }
  if (!packages.length) {
    return;
  }
  const args = ["tools/gates/rust-lints.mjs"];
  for (const pkg of packages) {
    args.push("--package", pkg.name);
  }
  run("node", args, {
    cwd: staged.mirror,
    env: {
      ...process.env,
      CARGO_TARGET_DIR: join(ROOT, "target"),
      GATE_ROOT: staged.mirror,
    },
  });
}

function runAst(phasePaths) {
  const rustPhase = phasePaths.filter((phasePath) =>
    RUST_FILE_EXT.test(phasePath),
  );
  if (rustPhase.length) {
    run(
      astGrep,
      [
        "scan",
        "--config",
        "sgconfig.yml",
        "--error",
        "--min-severity=error",
        "--max-results=1",
        "--inspect=summary",
        ...rustPhase,
      ],
      { cwd: staged.mirror },
    );
  }
}

function rustPackagesFor(phasePaths) {
  if (!phasePaths.some(isRustInput)) {
    return [];
  }
  return selectRustPackages(cargoMetadata(), phasePaths, staged.mirror);
}

function runAnalysisPhase(phasePaths, scope = "files") {
  runAnalysis({
    cargoPackages: rustPackagesFor(phasePaths),
    cwd: staged.mirror,
    paths: phasePaths,
    scope,
    toolRoot: ROOT,
  });
}

function runDomainAnalysis() {
  const repositoryPaths = listProjectFiles(staged.mirror);
  const domainPaths = selectDomainPaths(paths, repositoryPaths);
  runAnalysisPhase(domainPaths, "domain");
}

function runCodeGraphAffected(indexable) {
  if (!indexable.length) {
    return {
      fallback: "no indexable",
      inputs: [],
      paths: [],
      totalDependentsTraversed: 0,
    };
  }
  const result = run(
    codegraph,
    codeGraphAffectedArgs(staged.mirror, indexable),
    {
      allowFailure: true,
      capture: true,
      cwd: staged.mirror,
      quiet: true,
    },
  );
  if (result.status !== 0) {
    return {
      fallback: "CodeGraph affected query failed",
      inputs: indexable,
      paths: [],
      totalDependentsTraversed: 0,
    };
  }
  try {
    const parsed = JSON.parse(result.stdout);
    return {
      fallback: parsed.fallback,
      inputs: indexable,
      paths: parseAffectedJson(parsed),
      totalDependentsTraversed: parsed.totalDependentsTraversed ?? 0,
    };
  } catch {
    return {
      fallback: "CodeGraph returned invalid JSON",
      inputs: indexable,
      paths: [],
      totalDependentsTraversed: 0,
    };
  }
}

function listTestFilesUnder(directory) {
  const full = join(staged.mirror, directory);
  if (!existsSync(full)) {
    return [];
  }
  const out = [];
  function walk(currentDirectory) {
    for (const entry of readdirSync(currentDirectory, {
      withFileTypes: true,
    })) {
      const entryPath = join(currentDirectory, entry.name);
      if (entry.isDirectory()) {
        walk(entryPath);
      } else if (TEST_FILE_EXT.test(entry.name)) {
        out.push(relative(staged.mirror, entryPath));
      }
    }
  }
  walk(full);
  return out;
}

function domainTests(selection) {
  const tests = new Set();
  const domains = new Set([
    ...selection.stagedSources.map((record) => record.domain),
    ...selection.stagedTests.map((record) => record.domain),
  ]);
  for (const domain of domains) {
    if (domain === "tooling") {
      listTestFilesUnder("tools/gates/tests").forEach((testPath) =>
        tests.add(testPath),
      );
    } else if (domain === "plugin-release") {
      listTestFilesUnder("tools/release/tests").forEach((testPath) =>
        tests.add(testPath),
      );
    } else if (
      domain.startsWith("tests/environments/") ||
      domain.startsWith("tests/suites/")
    ) {
      listTestFilesUnder(domain).forEach((testPath) => tests.add(testPath));
    } else if (domain === "oracle") {
      listTestFilesUnder("tests/environments/oracle").forEach((testPath) =>
        tests.add(testPath),
      );
      listTestFilesUnder("tools/gates/tests").forEach((testPath) => {
        if (testPath.includes("oracle")) {
          tests.add(testPath);
        }
      });
    }
  }
  return [...tests].sort();
}

function writeSelectionReport(reportParams) {
  const { selection, graph, packages, nodeTests } = reportParams;
  const report = {
    codegraphCandidates: graph.paths,
    codegraphFallback: graph.fallback,
    codegraphInputs: graph.inputs,
    codegraphTotalDependentsTraversed: graph.totalDependentsTraversed ?? 0,
    domains: [
      ...new Set(
        [...selection.stagedSources, ...selection.stagedTests].map(
          (record) => record.domain,
        ),
      ),
    ],
    extendedTests: selection.extendedTests,
    languages: [
      ...new Set(
        [...selection.stagedSources, ...selection.stagedTests].map(
          (record) => record.language,
        ),
      ),
    ],
    nodeTestPaths: nodeTests,
    selectedRustPackages: packages.map((pkg) => pkg.name),
    stagedFiles: paths,
    stagedSources: selection.stagedSources,
    stagedTests: selection.stagedTests,
  };
  const reportDirectory = join(ROOT, ".cache", "gates");
  mkdirSync(reportDirectory, { recursive: true });
  writeFileSync(
    join(reportDirectory, "pre-commit-selection.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
}
function runPackageTests(packages) {
  if (!packages.length) {
    return;
  }
  const cargoEnv = {
    ...process.env,
    CARGO_TARGET_DIR: join(ROOT, "target"),
  };
  const packageFlags = packages.flatMap((pkg) => ["--package", pkg.name]);
  run(
    "cargo",
    ["test", ...packageFlags, "--all-targets", "--all-features", "--locked"],
    { cwd: staged.mirror, env: cargoEnv },
  );
  const libraries = packages.filter((pkg) => pkg.hasLibrary);
  if (libraries.length) {
    run(
      "cargo",
      [
        "test",
        ...libraries.flatMap((pkg) => ["--package", pkg.name]),
        "--doc",
        "--all-features",
        "--locked",
      ],
      { cwd: staged.mirror, env: cargoEnv },
    );
  }
}

function runTests() {
  const indexable = existing.filter(isCodeGraphIndexableSource);
  const graph = runCodeGraphAffected(indexable);
  const stagedSelection = computeSelectionRecord(paths);
  const ownedTests = domainTests(stagedSelection);
  const selection = computeSelectionRecord(paths, [
    ...graph.paths,
    ...ownedTests,
  ]);
  const rustInputs = [...paths, ...graph.paths].filter(isRustInput);
  const hasRustInput = rustInputs.length > 0;
  let packages = [];
  if (hasRustInput) {
    packages = selectRustPackages(cargoMetadata(), rustInputs, staged.mirror);
  }
  const directNode = [...selection.stagedTests, ...selection.extendedTests]
    .filter((record) => TEST_FILE_EXT.test(record.path))
    .map((record) => record.path)
    .filter((testPath) => existsSync(join(staged.mirror, testPath)));
  const nodeTests = [...new Set(directNode)].sort();
  writeSelectionReport({
    graph,
    nodeTests,
    packages,
    selection,
  });
  if (hasRustInput) {
    runPackageTests(packages);
  }
  const testEnvironment = {
    ...process.env,
    AST_GREP: astGrep,
    CODEGRAPH: codegraph,
    NODE_BIN: nodeBin,
    RUFF: ruff,
  };
  for (const testPath of nodeTests) {
    run("node", ["--test", testPath], {
      cwd: staged.mirror,
      env: testEnvironment,
    });
  }
}

const handlers = {
  "analysis-domain": runDomainAnalysis,
  "analysis-source": () => runAnalysisPhase(sourcePaths),
  "analysis-tests": () => runAnalysisPhase(testPaths),
  "ast-source": () => runAst(sourcePaths),
  "ast-tests": () => runAst(testPaths),
  "format-source": () => runFormat(sourcePaths),
  "format-tests": () => runFormat(testPaths),
  "lint-source": () => runLint(sourcePaths, "source"),
  "lint-tests": () => runLint(testPaths, "test"),
  structure: runStructure,
  test: runTests,
};

if (!handlers[mode]) {
  throw new Error(`unknown staged gate: ${mode ?? "<missing>"}`);
}
handlers[mode]();
