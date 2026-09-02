import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { output, run } from "../gates/lib/process.mjs";
import { packageSelection, parseAffectedJson } from "./affected.mjs";

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

const mode = process.argv[2];
const paths = staged.changes
  .flatMap((change) => [change.path, change.oldPath])
  .filter(Boolean);
const existing = [
  ...new Set(paths.filter((path) => existsSync(join(staged.mirror, path)))),
];
const rustFiles = existing.filter((path) => path.endsWith(".rs"));
const markdownFiles = existing.filter((path) => path.endsWith(".md"));
const formattedFiles = existing.filter((path) =>
  /(?:\.md|\.json|\.jsonc|\.ya?ml)$/.test(path),
);
const rustInputs = paths.filter(
  (path) =>
    path.endsWith(".rs") ||
    /(?:^|\/)Cargo\.(?:toml|lock)$/.test(path) ||
    path === "rust-toolchain.toml",
);
const nodeBin = process.env.NODE_BIN ?? join(ROOT, "node_modules", ".bin");
const tool = (name) => join(nodeBin, name);
const codegraph = process.env.CODEGRAPH ?? tool("codegraph");

function runStructure() {
  run("node", ["tools/gates/structure.mjs"], {
    cwd: staged.mirror,
    env: { ...process.env, GATE_ROOT: staged.mirror },
  });
  if (markdownFiles.length) {
    run(tool("markdownlint-cli2"), markdownFiles, { cwd: staged.mirror });
  }
}

function runFormat() {
  if (rustFiles.length || rustInputs.some((path) => path.includes("Cargo."))) {
    run("cargo", ["fmt", "--all", "--", "--check"], { cwd: staged.mirror });
  }
  if (formattedFiles.length) {
    run(tool("prettier"), ["--check", ...formattedFiles], {
      cwd: staged.mirror,
    });
  }
}

function runAst() {
  if (!rustFiles.length) return;
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

function runRust() {
  if (!rustInputs.length) return;
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
  if (!rustFiles.length) return { paths: [], fallback: undefined };
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
    { cwd: staged.mirror, capture: true, quiet: true, allowFailure: true },
  );
  if (result.status !== 0) {
    return { paths: [], fallback: "CodeGraph affected query failed" };
  }
  try {
    return {
      paths: parseAffectedJson(JSON.parse(result.stdout)),
      fallback: undefined,
    };
  } catch {
    return { paths: [], fallback: "CodeGraph returned invalid JSON" };
  }
}

function runTests() {
  const toolChange = paths.some((path) =>
    /^(?:tools\/(?:gates|hooks)\/|rules\/ast-grep\/|Makefile$|package(?:-lock)?\.json$|sgconfig\.yml$|\.husky\/)/.test(
      path,
    ),
  );
  if (toolChange) {
    run("npm", ["run", "test:tooling"], {
      cwd: staged.mirror,
      env: { ...process.env, CODEGRAPH: codegraph },
    });
  }
  if (!rustInputs.length) return;

  const graph = affectedCandidates();
  const cargo = cargoMetadata();
  const sharedInput = paths.some((path) =>
    /(?:^|\/)(?:Cargo\.(?:toml|lock)|rust-toolchain\.toml|build\.rs)$/.test(
      path,
    ),
  );
  let packages = sharedInput
    ? cargo.packages
        .filter((pkg) => cargo.workspace_members.includes(pkg.id))
        .map((pkg) => ({ name: pkg.name }))
    : packageSelection(cargo, [...paths, ...graph.paths], staged.mirror);
  let fallback = graph.fallback;
  if (!packages.length) {
    packages = cargo.packages
      .filter((pkg) => cargo.workspace_members.includes(pkg.id))
      .map((pkg) => ({ name: pkg.name }));
    fallback ??= "no changed path mapped to a Cargo package";
  }

  const report = {
    tree: staged.tree,
    changed: paths,
    codegraphCandidates: graph.paths,
    rustAnalyzer:
      "healthy diagnostics; CLI has no stable reference-batch contract",
    selectedPackages: packages.map((pkg) => pkg.name),
    fallback,
  };
  const reportDirectory = join(ROOT, ".cache", "gates");
  mkdirSync(reportDirectory, { recursive: true });
  writeFileSync(
    join(reportDirectory, "pre-commit-selection.json"),
    `${JSON.stringify(report, null, 2)}\n`,
  );

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

const handlers = {
  ast: runAst,
  format: runFormat,
  rust: runRust,
  structure: runStructure,
  test: runTests,
};
if (!handlers[mode])
  throw new Error(`unknown staged gate: ${mode ?? "<missing>"}`);
handlers[mode]();
