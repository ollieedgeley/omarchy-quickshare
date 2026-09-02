import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { output } from "./lib/process.mjs";

const SCRIPT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ROOT = resolve(process.env.GATE_ROOT ?? SCRIPT_ROOT);
const EXCLUDED_NAMES = new Set([
  ".cache",
  ".codegraph",
  ".git",
  "dist",
  "node_modules",
  "reports",
  "target",
]);
const GENERATED_FILES = new Set(["Cargo.lock", "package-lock.json"]);
const DIRECTORY_INDEXES = new Set(["crates", "tests/fixtures", "tests/suites"]);

function walkFiles(directory, base = directory) {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (EXCLUDED_NAMES.has(entry.name) || entry.name === "_") continue;
    const absolute = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...walkFiles(absolute, base));
    if (entry.isFile())
      files.push(relative(base, absolute).split(sep).join("/"));
  }
  return files;
}

function repositoryFiles() {
  if (!existsSync(join(ROOT, ".git"))) return walkFiles(ROOT);
  const listed = output(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
    { cwd: ROOT },
  );
  return listed ? listed.split("\0").filter(Boolean) : [];
}

function physicalLines(path) {
  const source = readFileSync(path, "utf8");
  if (!source) return 0;
  const lineBreaks = source.match(/\n/g)?.length ?? 0;
  return lineBreaks + (source.endsWith("\n") ? 0 : 1);
}

export function lineBudgetFailures(files) {
  const failures = [];
  for (const path of files) {
    if (
      GENERATED_FILES.has(path) ||
      path.startsWith("rules/ast-grep/schemas/")
    ) {
      continue;
    }
    const absolute = join(ROOT, path);
    if (!existsSync(absolute) || !statSync(absolute).isFile()) continue;
    const isTest =
      path.includes("/tests/") || /(?:^|\.)test\.[^.]+$/.test(path);
    const limit = isTest ? 800 : 500;
    const lines = physicalLines(absolute);
    if (lines > limit)
      failures.push(`${path}: ${lines} lines (limit ${limit})`);
  }
  return failures;
}

export function directoryBudgetFailures(files) {
  const contents = new Map();
  for (const path of files) {
    const parts = path.split("/");
    for (let index = 0; index < parts.length; index += 1) {
      const directory = parts.slice(0, index).join("/") || ".";
      const child = parts[index];
      const kind = index === parts.length - 1 ? "files" : "directories";
      const entry = contents.get(directory) ?? {
        files: new Set(),
        directories: new Set(),
      };
      entry[kind].add(child);
      contents.set(directory, entry);
    }
  }

  const failures = [];
  for (const [directory, entry] of contents) {
    if (directory !== "." && entry.files.size > 12) {
      failures.push(
        `${directory}: ${entry.files.size} direct files (limit 12)`,
      );
    }
    if (
      entry.directories.size > 12 &&
      !DIRECTORY_INDEXES.has(directory === "." ? "" : directory)
    ) {
      failures.push(
        `${directory}: ${entry.directories.size} direct directories (limit 12)`,
      );
    }
  }
  return failures;
}

function configurationFailures() {
  const failures = [];
  const packageJson = JSON.parse(
    readFileSync(join(ROOT, "package.json"), "utf8"),
  );
  const requiredPins = {
    "@ast-grep/cli": "0.45.3",
    "@colbymchenry/codegraph": "1.6.0",
    husky: "9.1.7",
    "markdownlint-cli2": "0.23.2",
    prettier: "3.9.6",
  };
  for (const [name, version] of Object.entries(requiredPins)) {
    if (packageJson.devDependencies?.[name] !== version) {
      failures.push(`package.json must pin ${name} to ${version}`);
    }
  }

  const toolchain = readFileSync(join(ROOT, "rust-toolchain.toml"), "utf8");
  if (!toolchain.includes('channel = "1.98.0"')) {
    failures.push("rust-toolchain.toml must pin Rust 1.98.0");
  }
  for (const component of ["clippy", "rust-analyzer", "rustfmt"]) {
    if (!toolchain.includes(component))
      failures.push(`missing Rust component: ${component}`);
  }

  const astConfig = readFileSync(join(ROOT, "sgconfig.yml"), "utf8");
  for (const key of ["ruleDirs:", "testConfigs:", "utilDirs:"]) {
    if (!astConfig.includes(key)) failures.push(`sgconfig.yml missing ${key}`);
  }
  if (existsSync(join(ROOT, ".github/workflows"))) {
    failures.push("hosted CI is out of scope; remove .github/workflows");
  }
  return failures;
}

export function checkStructure() {
  const files = repositoryFiles();
  return [
    ...lineBudgetFailures(files),
    ...directoryBudgetFailures(files),
    ...configurationFailures(),
  ];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const failures = checkStructure();
  if (failures.length) {
    process.stderr.write(`${failures.join("\n")}\n`);
    process.exitCode = 1;
  } else {
    process.stdout.write("Repository structure contracts passed.\n");
  }
}
