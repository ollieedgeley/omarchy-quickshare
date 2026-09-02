import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { output, run } from "../gates/lib/process.mjs";
import { structureScope } from "../gates/structure.mjs";
import { packageSelection, parseAffectedJson } from "./affected.mjs";

const ROOT = output("git", ["rev-parse", "--show-toplevel"]);
const metadataPath = join(ROOT, ".cache", "gates", "staged.json");
if (!existsSync(metadataPath)) {
  throw new Error("staged metadata is missing; run `make pre-commit-prepare`");
}
const staged = JSON.parse(readFileSync(metadataPath, "utf8"));
const currentTree = output("git", ["write-tree"], { cwd: ROOT });
const JAVASCRIPT_EXTENSION_PATTERN = /\.(?:c|m)?js$/u;
const FORMATTED_EXTENSION_PATTERN =
  /(?:\.(?:c|m)?js|\.json|\.jsonc|\.md|\.ya?ml)$/u;
const CARGO_MANIFEST_PATTERN = /(?:^|\/)Cargo\.(?:lock|toml)$/u;
const SHARED_RUST_INPUT_PATTERN =
  /(?:^|\/)(?:build\.rs|Cargo\.(?:lock|toml)|rust-toolchain\.toml)$/u;
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
const rustFiles = existing.filter((path) => path.endsWith(".rs"));
const markdownFiles = existing.filter((path) => path.endsWith(".md"));
const javascriptFiles = existing.filter((path) =>
  JAVASCRIPT_EXTENSION_PATTERN.test(path),
);
const formattedFiles = existing.filter((path) =>
  FORMATTED_EXTENSION_PATTERN.test(path),
);
const rustInputs = paths.filter(
  (path) =>
    path.endsWith(".rs") ||
    CARGO_MANIFEST_PATTERN.test(path) ||
    ["clippy.toml", "rust-toolchain.toml", "rustfmt.toml"].includes(path),
);
const nodeBin = process.env.NODE_BIN ?? join(ROOT, "node_modules", ".bin");
const tool = (name) => join(nodeBin, name);
const codegraph = process.env.CODEGRAPH ?? tool("codegraph");

function runStructure() {
  const scopes = new Set(paths.map(structureScope));
  for (const scope of scopes) {
    run("node", ["tools/gates/structure.mjs", scope], {
      cwd: staged.mirror,
      env: {
        ...process.env,
        AST_GREP: tool("ast-grep"),
        GATE_ROOT: staged.mirror,
      },
    });
  }
  if (markdownFiles.length) {
    run(tool("markdownlint-cli2"), markdownFiles, { cwd: staged.mirror });
  }
}

function runFormat() {
  const cargoOrFormatConfig = rustInputs.some(
    (path) => path.includes("Cargo.") || path === "rustfmt.toml",
  );
  if (rustFiles.length || cargoOrFormatConfig) {
    run("cargo", ["fmt", "--all", "--", "--check"], { cwd: staged.mirror });
  }
  if (formattedFiles.length) {
    run(tool("prettier"), ["--check", ...formattedFiles], {
      cwd: staged.mirror,
    });
  }
}

function runAst() {
  if (!rustFiles.length) {
    return;
  }
  run(
    tool("ast-grep"),
    [
      "scan",
      "--config",
      "sgconfig.yml",
      "--error",
      "--min-severity=error",
      "--max-results=1",
      "--inspect=summary",
      ...rustFiles,
    ],
    { cwd: staged.mirror },
  );
}

function runJavascript() {
  if (!javascriptFiles.length) {
    return;
  }
  run(
    tool("eslint"),
    ["--max-warnings", "0", "--no-warn-ignored", ...javascriptFiles],
    { cwd: staged.mirror },
  );
}

function runRust() {
  if (!rustInputs.length) {
    return;
  }
  run("node", ["tools/gates/rust-lints.mjs"], {
    cwd: staged.mirror,
    env: { ...process.env, GATE_ROOT: staged.mirror },
  });
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

function affectedCandidates() {
  if (!rustFiles.length) {
    return { paths: [] };
  }
  const result = run(
    codegraph,
    [
      "affected",
      "--path",
      staged.mirror,
      "--depth",
      "32",
      "--filter",
      "**/*.rs",
      "--json",
      ...rustFiles,
    ],
    {
      allowFailure: true,
      capture: true,
      cwd: staged.mirror,
      quiet: true,
    },
  );
  if (result.status !== 0) {
    return { paths: [], fallback: "CodeGraph affected query failed" };
  }
  try {
    return {
      paths: parseAffectedJson(JSON.parse(result.stdout)),
    };
  } catch {
    return { paths: [], fallback: "CodeGraph returned invalid JSON" };
  }
}

function isToolingPath(path) {
  const prefixes = [
    ".husky/",
    "rules/ast-grep/",
    "tests/environments/",
    "tools/gates/",
    "tools/hooks/",
  ];
  const files = new Set([
    ".prettierrc.json",
    "Makefile",
    "clippy.toml",
    "eslint.config.mjs",
    "package-lock.json",
    "package.json",
    "rustfmt.toml",
    "sgconfig.yml",
  ]);
  return files.has(path) || prefixes.some((prefix) => path.startsWith(prefix));
}

function toolingTests() {
  const toolChange = paths.some(isToolingPath);
  if (toolChange) {
    run("npm", ["run", "test:tooling"], {
      cwd: staged.mirror,
      env: {
        ...process.env,
        AST_GREP: tool("ast-grep"),
        CODEGRAPH: codegraph,
        NODE_BIN: nodeBin,
      },
    });
  }
}

function workspacePackages(cargo) {
  return cargo.packages
    .filter((pkg) => cargo.workspace_members.includes(pkg.id))
    .map((pkg) => ({ name: pkg.name }));
}

function selectPackages(cargo, graph) {
  const sharedInput = paths.some((path) =>
    SHARED_RUST_INPUT_PATTERN.test(path),
  );
  if (sharedInput) {
    return workspacePackages(cargo);
  }
  return packageSelection(cargo, [...paths, ...graph.paths], staged.mirror);
}

function writeSelectionReport(graph, packages, fallback) {
  const report = {
    changed: paths,
    codegraphCandidates: graph.paths,
    fallback,
    rustAnalyzer:
      "healthy diagnostics; CLI has no stable reference-batch contract",
    selectedPackages: packages.map((pkg) => pkg.name),
    tree: staged.tree,
  };
  const reportDirectory = join(ROOT, ".cache", "gates");
  mkdirSync(reportDirectory, { recursive: true });
  writeFileSync(
    join(reportDirectory, "pre-commit-selection.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );
}

function runPackageTests(packages) {
  for (const pkg of packages) {
    run(
      "cargo",
      [
        "test",
        "--package",
        pkg.name,
        "--all-targets",
        "--all-features",
        "--locked",
      ],
      { cwd: staged.mirror },
    );
    run(
      "cargo",
      ["test", "--package", pkg.name, "--doc", "--all-features", "--locked"],
      { cwd: staged.mirror },
    );
  }
}

function runTests() {
  toolingTests();
  if (!rustInputs.length) {
    return;
  }

  const graph = affectedCandidates();
  const cargo = cargoMetadata();
  let packages = selectPackages(cargo, graph);
  let { fallback } = graph;
  if (!packages.length) {
    packages = workspacePackages(cargo);
    fallback ??= "no changed path mapped to a Cargo package";
  }
  writeSelectionReport(graph, packages, fallback);
  runPackageTests(packages);
}

const handlers = {
  ast: runAst,
  format: runFormat,
  javascript: runJavascript,
  rust: runRust,
  structure: runStructure,
  test: runTests,
};
if (!handlers[mode]) {
  throw new Error(`unknown staged gate: ${mode ?? "<missing>"}`);
}
handlers[mode]();
