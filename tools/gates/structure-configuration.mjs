import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ESLint } from "eslint";

const SCRIPT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ROOT = resolve(process.env.GATE_ROOT ?? SCRIPT_ROOT);
const CODE_LINE_LIMIT = 80;
const FUNCTION_LINE_LIMIT = 50;
const ESLINT_ERROR_SEVERITY = 2;

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
