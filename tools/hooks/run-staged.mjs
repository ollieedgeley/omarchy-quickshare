import { existsSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";

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
  partitionRustTestPackages,
  selectRustPackages,
} from "./affected.mjs";
import { planAffectedGates } from "./test-gates.mjs";
import { updateReport } from "./records.mjs";
import {
  cachedSelection,
  executionEnvironment,
  recordedStep,
  runIntegrations,
  verifySnapshot,
} from "./run-staged/execution.mjs";

const ROOT = output("git", ["rev-parse", "--show-toplevel"]);
const metadataPath = join(ROOT, ".cache", "gates", "staged.json");
if (!existsSync(metadataPath)) {
  throw new Error("staged metadata is missing; run `make pre-commit-prepare`");
}
const staged = JSON.parse(readFileSync(metadataPath, "utf8"));

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

const nodeBin = resolve(ROOT, process.env.NODE_BIN ?? "node_modules/.bin");
const tool = (name) => join(nodeBin, name);
const codegraph = resolve(ROOT, process.env.CODEGRAPH ?? tool("codegraph"));
const ruff = resolve(ROOT, process.env.RUFF ?? ".cache/tools/ruff-0.16.5/ruff");
const astGrep = resolve(ROOT, process.env.AST_GREP ?? tool("ast-grep"));
const JS_SOURCE_EXT = /\.[cm]?[jt]sx?$/u;
const PRETTIER_SUPPORTED_EXT = /\.(?:[cm]?[jt]sx?|jsonc?|markdown|md|ya?ml)$/u;
const PYTHON_EXT = /\.pyi?$/u;
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

function writeSelectionReport(reportParams) {
  const { selection, graph, packages, nodeTests, gates, rustInputs } =
    reportParams;
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
    gates,
    languages: [
      ...new Set(
        [...selection.stagedSources, ...selection.stagedTests].map(
          (record) => record.language,
        ),
      ),
    ],
    nodeTestPaths: nodeTests,
    packages,
    rustInputs,
    selectedRustPackages: packages.map((pkg) => pkg.name),
    stagedFiles: paths,
    stagedSources: selection.stagedSources,
    stagedTests: selection.stagedTests,
  };
  updateReport(staged.reportPath, { selection: report });
  return report;
}
function testEnvironment() {
  return executionEnvironment(staged, {
    AST_GREP: astGrep,
    CODEGRAPH: codegraph,
    NODE_BIN: nodeBin,
    RUFF: ruff,
  });
}
function runPackageTests(packages) {
  if (!packages.length) {
    return;
  }
  const packageFlags = packages.flatMap((pkg) => ["--package", pkg.name]);
  recordedStep(staged, {
    args: [
      "test",
      ...packageFlags,
      "--all-targets",
      "--all-features",
      "--locked",
    ],
    command: "cargo",
    env: testEnvironment(),
    id: `cargo-${mode}`,
    kind: "cargo",
    reasons: packages.map((pkg) => ({ kind: "cargo-package", path: pkg.name })),
  });
  const libraries = packages.filter((pkg) => pkg.hasLibrary);
  if (libraries.length) {
    recordedStep(staged, {
      args: [
        "test",
        ...libraries.flatMap((pkg) => ["--package", pkg.name]),
        "--doc",
        "--all-features",
        "--locked",
      ],
      command: "cargo",
      env: testEnvironment(),
      id: `cargo-doc-${mode}`,
      kind: "cargo",
      reasons: libraries.map((pkg) => ({
        kind: "cargo-package",
        path: pkg.name,
      })),
    });
  }
}

function selectTests() {
  const indexable = existing.filter(isCodeGraphIndexableSource);
  const graph = runCodeGraphAffected(indexable);
  updateReport(staged.reportPath, { selectionInputs: { graph, paths } });
  const {
    gates,
    fastNodeTests: nodeTests,
    rustInputs: mappedRustInputs,
  } = planAffectedGates({
    affectedTests: graph.paths,
    changedPaths: paths,
    repositoryFiles: listProjectFiles(staged.mirror),
  });
  const rustInputs = [
    ...new Set(
      [...paths, ...graph.paths, ...mappedRustInputs].filter(isRustInput),
    ),
  ];
  let packages = [];
  if (rustInputs.length) {
    packages = selectRustPackages(cargoMetadata(), rustInputs, staged.mirror);
  }
  const selection = computeSelectionRecord(paths, [
    ...graph.paths,
    ...nodeTests,
    ...mappedRustInputs,
    ...gates.flatMap((gate) => gate.testPaths),
  ]);
  return writeSelectionReport({
    gates,
    graph,
    nodeTests,
    packages,
    rustInputs,
    selection,
  });
}

function runTests(scope) {
  const selection = cachedSelection(staged) ?? selectTests();
  if (scope === "integrations") {
    runIntegrations(staged, selection.gates, testEnvironment());
  } else if (scope === "tooling") {
    for (const testPath of selection.nodeTestPaths) {
      recordedStep(staged, {
        args: ["--test", testPath],
        command: process.execPath,
        env: testEnvironment(),
        id: testPath,
        kind: "node",
        reasons: [{ kind: "selected-test", path: testPath }],
      });
    }
  } else {
    runPackageTests(partitionRustTestPackages(selection.packages)[scope]);
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
  "test-app": () => runTests("app"),
  "test-integrations": () => runTests("integrations"),
  "test-libraries": () => runTests("libraries"),
  "test-tooling": () => runTests("tooling"),
};

if (!handlers[mode]) {
  throw new Error(`unknown staged gate: ${mode ?? "<missing>"}`);
}
try {
  verifySnapshot(staged);
  handlers[mode]();
} catch (error) {
  updateReport(staged.reportPath, {
    failure: { message: error.message, phase: mode },
    status: "failed",
  });
  throw error;
}
