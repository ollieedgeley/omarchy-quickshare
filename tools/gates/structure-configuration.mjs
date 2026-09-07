import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { ESLint } from "eslint";

const SCRIPT_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const ROOT = resolve(process.env.GATE_ROOT ?? SCRIPT_ROOT);
const CODE_LINE_LIMIT = 80;
const FUNCTION_LINE_LIMIT = 50;
const ESLINT_ERROR_SEVERITY = 2;

function rustConfigurationFailures() {
  const failures = [];
  const toolchain = readFileSync(join(ROOT, "rust-toolchain.toml"), "utf8");
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
    failures.push(...(await javascriptConfigurationFailures()));
  }
  if (scope !== "tooling") {
    failures.push(...rustConfigurationFailures());
    failures.push(...astConfigurationFailures());
  }
  return failures;
}
