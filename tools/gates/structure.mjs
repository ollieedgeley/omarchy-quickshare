import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

import { ESLint } from "eslint";

import { output, run } from "./lib/process.mjs";

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
const APPLICATION_PREFIXES = [
  "crates/",
  "fuzz/",
  "packaging/",
  "rules/ast-grep/",
];
const APPLICATION_FILES = new Set([
  "Cargo.lock",
  "Cargo.toml",
  "clippy.toml",
  "rust-toolchain.toml",
  "rustfmt.toml",
  "sgconfig.yml",
]);
const PROJECT_LINE_LIMIT = 500;
const TEST_LINE_LIMIT = 800;
const DIRECTORY_CHILD_LIMIT = 12;
const CODE_LINE_LIMIT = 80;
const FUNCTION_LINE_LIMIT = 50;
const ESLINT_ERROR_SEVERITY = 2;
const AST_GREP = resolve(
  process.env.AST_GREP ?? join(SCRIPT_ROOT, "node_modules/.bin/ast-grep"),
);
const CODE_EXTENSION = /\.(?:cjs|js|mjs|py|pyi|qml|rs|sh)$/u;
const NATIVE_EXTENSION = /\.(?:c|cc|cpp|cxx|h|hh|hpp|hxx|inl|ipp)$/u;
const JVM_EXTENSION = /\.(?:java|kt|kts)$/u;
const BUILD_EXTENSION = /\.(?:bazel|bzl|dockerfile)$/u;
const BUILD_FILE =
  /(?:^|\/)(?:BUILD(?:\.bazel)?|Dockerfile(?:\.[^/]+)?|MODULE\.bazel)$/u;
const BASH_SHEBANG = /^#!.*(?:\/|\s)bash(?:\s|$)/u;
const FUNCTION_KINDS = new Map([
  ["bash", ["function_definition"]],
  ["c", ["function_definition"]],
  ["cpp", ["function_definition"]],
  ["java", ["method_declaration", "constructor_declaration"]],
  ["kotlin", ["function_declaration", "secondary_constructor"]],
  ["python", ["function_definition"]],
  ["qml", ["function_declaration"]],
]);

function walkFiles(directory, base = directory) {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!EXCLUDED_NAMES.has(entry.name) && entry.name !== "_") {
      const absolute = join(directory, entry.name);
      if (entry.isDirectory()) {
        files.push(...walkFiles(absolute, base));
      }
      if (entry.isFile()) {
        files.push(relative(base, absolute).split(sep).join("/"));
      }
    }
  }
  return files;
}

function repositoryFiles() {
  if (!existsSync(join(ROOT, ".git"))) {
    return walkFiles(ROOT);
  }
  const listed = output(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
    { cwd: ROOT },
  );
  if (listed) {
    return listed.split("\0").filter(Boolean);
  }
  return [];
}

function physicalLines(path) {
  const source = readFileSync(path, "utf8");
  if (!source) {
    return 0;
  }
  const lineBreaks = source.match(/\n/gu)?.length ?? 0;
  let finalLine = 1;
  if (source.endsWith("\n")) {
    finalLine = 0;
  }
  return lineBreaks + finalLine;
}

export function lineLimit(path) {
  const isTest =
    path.startsWith("tests/") ||
    path.includes("/tests/") ||
    /(?:^|\.)test\.[^.]+$/u.test(path);
  if (isTest) {
    return TEST_LINE_LIMIT;
  }
  return PROJECT_LINE_LIMIT;
}

export function structureScope(path) {
  if (
    APPLICATION_FILES.has(path) ||
    APPLICATION_PREFIXES.some((prefix) => path.startsWith(prefix))
  ) {
    return "app";
  }
  return "tooling";
}

export function lineBudgetFailures(files) {
  const failures = [];
  for (const path of files) {
    const isGenerated =
      GENERATED_FILES.has(path) || path.startsWith("rules/ast-grep/schemas/");
    if (!isGenerated) {
      const absolute = join(ROOT, path);
      if (existsSync(absolute) && statSync(absolute).isFile()) {
        const limit = lineLimit(path);
        const lines = physicalLines(absolute);
        if (lines > limit) {
          failures.push(`${path}: ${lines} lines (limit ${limit})`);
        }
      }
    }
  }
  return failures;
}

function isMakefile(path) {
  return path === "Makefile" || path.endsWith("/Makefile");
}

function isBashFile(path, source) {
  return path.endsWith(".sh") || BASH_SHEBANG.test(source);
}

function isCodeFile(path, source) {
  return (
    CODE_EXTENSION.test(path) ||
    NATIVE_EXTENSION.test(path) ||
    JVM_EXTENSION.test(path) ||
    BUILD_EXTENSION.test(path) ||
    BUILD_FILE.test(path) ||
    isMakefile(path) ||
    isBashFile(path, source)
  );
}

function functionLanguage(path, source) {
  if (isBashFile(path, source)) {
    return "bash";
  }
  if (/\.pyi?$/u.test(path)) {
    return "python";
  }
  if (path.endsWith(".c")) {
    return "c";
  }
  if (/\.(?:cc|cpp|cxx|h|hh|hpp|hxx|inl|ipp)$/u.test(path)) {
    return "cpp";
  }
  if (path.endsWith(".java")) {
    return "java";
  }
  if (/\.kts?$/u.test(path)) {
    return "kotlin";
  }
  if (path.endsWith(".qml")) {
    return "qml";
  }
  return null;
}

export function codeLineFailures(path, source) {
  if (!isCodeFile(path, source)) {
    return [];
  }
  const failures = [];
  for (const [index, line] of source.split("\n").entries()) {
    const { length } = [...line];
    if (length > CODE_LINE_LIMIT) {
      failures.push(
        `${path}:${index + 1}: ${length} columns (limit ${CODE_LINE_LIMIT})`,
      );
    }
  }
  return failures;
}

function functionMatchFailure(match, language, reportedFile) {
  const start = match.range.start.line;
  const lines = match.range.end.line - start + 1;
  if (lines <= FUNCTION_LINE_LIMIT) {
    return [];
  }
  const path = reportedFile ?? match.file;
  return [
    `${path}:${start + 1}: ${language} function has ${lines} lines ` +
      `(limit ${FUNCTION_LINE_LIMIT})`,
  ];
}

function astFunctionFailures(specification, options) {
  const { files, input, kind, language, reportedFile } = specification;
  if (!files.length) {
    return [];
  }
  const root = options.root ?? ROOT;
  const astGrep = options.astGrep ?? AST_GREP;
  let parserLanguage = language;
  if (language === "qml") {
    parserLanguage = "javascript";
  }
  const astArguments = [
    "run",
    "--kind",
    kind,
    "--lang",
    parserLanguage,
    "--json=stream",
  ];
  astArguments.push(...files);
  const result = run(astGrep, astArguments, {
    allowFailure: true,
    capture: true,
    cwd: root,
    input,
    quiet: true,
  });
  const errorDetail = result.stderr.trim();
  if (result.status > 1 || errorDetail) {
    const detail = errorDetail || `ast-grep exited with ${result.status}`;
    throw new Error(detail);
  }
  const matches = result.stdout.trim();
  if (!matches) {
    return [];
  }
  return matches
    .split("\n")
    .map((line) => JSON.parse(line))
    .flatMap((match) => functionMatchFailure(match, language, reportedFile));
}

export function functionSpanFailures(filesByLanguage, options = {}) {
  const failures = [];
  for (const [language, files] of filesByLanguage) {
    const kinds = FUNCTION_KINDS.get(language) ?? [];
    for (const kind of kinds) {
      if (language === "qml") {
        const root = options.root ?? ROOT;
        for (const path of files) {
          const input = readFileSync(join(root, path), "utf8");
          const specification = {
            files: ["--stdin"],
            input,
            kind,
            language,
            reportedFile: path,
          };
          failures.push(...astFunctionFailures(specification, options));
        }
      } else {
        failures.push(
          ...astFunctionFailures({ files, kind, language }, options),
        );
      }
    }
  }
  return failures;
}

export function codeMetricFailures(files) {
  const failures = [];
  const filesByLanguage = new Map();
  for (const path of files) {
    const absolute = join(ROOT, path);
    if (existsSync(absolute) && statSync(absolute).isFile()) {
      const source = readFileSync(absolute, "utf8");
      failures.push(...codeLineFailures(path, source));
      const language = functionLanguage(path, source);
      if (language) {
        const languageFiles = filesByLanguage.get(language) ?? [];
        languageFiles.push(path);
        filesByLanguage.set(language, languageFiles);
      }
    }
  }
  return [...failures, ...functionSpanFailures(filesByLanguage)];
}

function isDirectoryIndex(directory) {
  if (directory === ".") {
    return DIRECTORY_INDEXES.has("");
  }
  return DIRECTORY_INDEXES.has(directory);
}

export function directoryBudgetFailures(files) {
  const contents = new Map();
  for (const path of files) {
    const parts = path.split("/");
    for (let index = 0; index < parts.length; index += 1) {
      let directory = parts.slice(0, index).join("/");
      if (!directory) {
        directory = ".";
      }
      const child = parts[index];
      let kind = "directories";
      if (index === parts.length - 1) {
        kind = "files";
      }
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
    if (directory !== "." && entry.files.size > DIRECTORY_CHILD_LIMIT) {
      failures.push(
        `${directory}: ${entry.files.size} direct files (limit 12)`,
      );
    }
    if (
      entry.directories.size > DIRECTORY_CHILD_LIMIT &&
      !isDirectoryIndex(directory)
    ) {
      failures.push(
        `${directory}: ${entry.directories.size} direct directories (limit 12)`,
      );
    }
  }
  return failures;
}

function packageConfigurationFailures() {
  const failures = [];
  const packageJson = JSON.parse(
    readFileSync(join(ROOT, "package.json"), "utf8"),
  );
  const requiredPins = {
    "@ast-grep/cli": "0.45.3",
    "@colbymchenry/codegraph": "1.6.0",
    "@eslint/js": "10.0.1",
    "@stylistic/eslint-plugin": "5.10.0",
    eslint: "10.9.1",
    globals: "17.12.0",
    husky: "9.1.7",
    "markdownlint-cli2": "0.23.2",
    prettier: "3.9.6",
  };
  for (const [name, version] of Object.entries(requiredPins)) {
    if (packageJson.devDependencies?.[name] !== version) {
      failures.push(`package.json must pin ${name} to ${version}`);
    }
  }
  return failures;
}

function rustConfigurationFailures() {
  const failures = [];
  const toolchain = readFileSync(join(ROOT, "rust-toolchain.toml"), "utf8");
  if (!toolchain.includes('channel = "1.98.0"')) {
    failures.push("rust-toolchain.toml must pin Rust 1.98.0");
  }
  for (const component of ["clippy", "rust-analyzer", "rustfmt"]) {
    if (!toolchain.includes(component)) {
      failures.push(`missing Rust component: ${component}`);
    }
  }
  const rustfmt = readFileSync(join(ROOT, "rustfmt.toml"), "utf8");
  if (!rustfmt.includes(`max_width = ${CODE_LINE_LIMIT}`)) {
    failures.push(`rustfmt.toml must set max_width to ${CODE_LINE_LIMIT}`);
  }
  const clippy = readFileSync(join(ROOT, "clippy.toml"), "utf8");
  const threshold = `too-many-lines-threshold = ${FUNCTION_LINE_LIMIT}`;
  if (!clippy.includes(threshold)) {
    failures.push(
      `clippy.toml must set too-many-lines-threshold to ${FUNCTION_LINE_LIMIT}`,
    );
  }
  return failures;
}

function astConfigurationFailures() {
  const failures = [];
  const astConfig = readFileSync(join(ROOT, "sgconfig.yml"), "utf8");
  for (const key of ["ruleDirs:", "testConfigs:", "utilDirs:"]) {
    if (!astConfig.includes(key)) {
      failures.push(`sgconfig.yml missing ${key}`);
    }
  }
  if (existsSync(join(ROOT, ".github/workflows"))) {
    failures.push("hosted CI is out of scope; remove .github/workflows");
  }
  return failures;
}

async function javascriptConfigurationFailures() {
  const failures = [];
  const prettier = JSON.parse(
    readFileSync(join(ROOT, ".prettierrc.json"), "utf8"),
  );
  if (prettier.printWidth !== CODE_LINE_LIMIT) {
    failures.push(`Prettier printWidth must be ${CODE_LINE_LIMIT}`);
  }
  const eslint = new ESLint({ cwd: ROOT });
  const config = await eslint.calculateConfigForFile(
    "tools/gates/structure.mjs",
  );
  const lineRule = config.rules["@stylistic/max-len"];
  if (
    lineRule[0] !== ESLINT_ERROR_SEVERITY ||
    lineRule[1].code !== CODE_LINE_LIMIT ||
    lineRule[1].comments !== CODE_LINE_LIMIT
  ) {
    failures.push(`resolved ESLint max-len must be ${CODE_LINE_LIMIT}`);
  }
  const functionRule = config.rules["max-lines-per-function"];
  if (
    functionRule[0] !== ESLINT_ERROR_SEVERITY ||
    functionRule[1].max !== FUNCTION_LINE_LIMIT
  ) {
    failures.push(
      `resolved ESLint function length must be ${FUNCTION_LINE_LIMIT}`,
    );
  }
  return failures;
}

export async function configurationFailures(scope = "all") {
  const failures = [];
  if (scope !== "app") {
    failures.push(...packageConfigurationFailures());
    failures.push(...(await javascriptConfigurationFailures()));
  }
  if (scope !== "tooling") {
    failures.push(...rustConfigurationFailures());
    failures.push(...astConfigurationFailures());
  }
  return failures;
}

export async function checkStructure(scope = "all") {
  if (!["all", "app", "tooling"].includes(scope)) {
    throw new Error(`unknown structure scope: ${scope}`);
  }
  let files = repositoryFiles();
  if (scope !== "all") {
    files = files.filter((path) => structureScope(path) === scope);
  }
  return [
    ...lineBudgetFailures(files),
    ...directoryBudgetFailures(files),
    ...codeMetricFailures(files),
    ...(await configurationFailures()),
  ];
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const scope = process.argv[2] ?? "all";
  const failures = await checkStructure(scope);
  if (failures.length) {
    process.stderr.write(`${failures.join("\n")}\n`);
    process.exitCode = 1;
  } else {
    process.stdout.write(`Repository ${scope} structure contracts passed.\n`);
  }
}
