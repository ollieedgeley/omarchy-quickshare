import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { output, run } from "./lib/process.mjs";
export function parsePackageArgs(args = []) {
  const packages = [];
  let index = 0;
  while (index < args.length) {
    const arg = args[index];
    if (arg === "--package" || arg === "-p") {
      index += 1;
      const name = args[index];
      if (!name || name.startsWith("-")) {
        throw new Error("malformed or empty --package argument");
      }
      packages.push(name);
      index += 1;
    } else {
      throw new Error(`unknown or positional argument: ${arg}`);
    }
  }
  // Deterministic dedupe preserves first-seen order.
  const seen = new Set();
  return packages.filter(
    (packageName) => !seen.has(packageName) && seen.add(packageName),
  );
}

const isDirectExecution =
  import.meta.url === pathToFileURL(process.argv[1]).href;

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const MINIMUM_EXCEPTION_REASON_LENGTH = 20;
const ALLOWED_RUSTC_LINT_PATTERN = /^\s+(?<lint>[a-z0-9-]+)\s+allow\s+/gmu;
const exceptions = JSON.parse(
  readFileSync(join(ROOT, "tools/gates/clippy-exceptions.json"), "utf8"),
);
const rustcExceptions = JSON.parse(
  readFileSync(join(ROOT, "tools/gates/rustc-exceptions.json"), "utf8"),
);

for (const [lint, reason] of Object.entries(exceptions)) {
  if (
    !lint ||
    typeof reason !== "string" ||
    reason.trim().length < MINIMUM_EXCEPTION_REASON_LENGTH
  ) {
    throw new Error(
      `Clippy exception ${lint || "<empty>"} needs a specific reason`,
    );
  }
}
for (const [lint, reason] of Object.entries(rustcExceptions)) {
  if (
    !lint ||
    typeof reason !== "string" ||
    reason.trim().length < MINIMUM_EXCEPTION_REASON_LENGTH
  ) {
    throw new Error(
      `rustc exception ${lint || "<empty>"} needs a specific reason`,
    );
  }
}

if (isDirectExecution) {
  const cliPackages = parsePackageArgs(process.argv.slice(2));
  const useWorkspace = cliPackages.length === 0;
  const packageFlags = [];
  if (useWorkspace) {
    packageFlags.push("--workspace");
  } else {
    packageFlags.push(
      ...cliPackages.flatMap((packageName) => ["-p", packageName]),
    );
  }

  const rustcVersion = output("rustc", ["--version"], { cwd: ROOT });
  const analyzerVersion = output("rust-analyzer", ["--version"], { cwd: ROOT });
  if (!rustcVersion.includes("1.98.0")) {
    throw new Error(`expected Rust 1.98.0, received ${rustcVersion}`);
  }
  if (!analyzerVersion.includes("1.98.0")) {
    throw new Error(
      `expected rust-analyzer 1.98.0, received ${analyzerVersion}`,
    );
  }

  const rustcHelp = output("rustc", ["-W", "help"], { cwd: ROOT });
  const allowedRustcLints = [
    ...rustcHelp.matchAll(ALLOWED_RUSTC_LINT_PATTERN),
  ].map((match) => match.groups.lint);
  const cargoManifest = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const missingRustcLints = allowedRustcLints.filter((lint) => {
    const manifestName = lint.replaceAll("-", "_");
    const manifestPattern = new RegExp(
      `^${manifestName} = "(?:deny|forbid)"$`,
      "mu",
    );
    const enabled = manifestPattern.test(cargoManifest);
    return !enabled && !Object.hasOwn(rustcExceptions, lint);
  });
  if (missingRustcLints.length) {
    throw new Error(
      [
        "unaccounted rustc allow-by-default lints:",
        missingRustcLints.join(", "),
      ].join(" "),
    );
  }

  const step = process.env.RUST_LINT_STEP ?? "clippy";
  if (step !== "clippy" && step !== "docs" && step !== "analyzer") {
    throw new Error(`unknown RUST_LINT_STEP: ${step}`);
  }
  if (step === "analyzer") {
    if (useWorkspace) {
      run("rust-analyzer", ["diagnostics", ".", "--severity", "warning"], {
        cwd: ROOT,
      });
    }
  } else if (step === "docs") {
    if (useWorkspace) {
      run(
        "cargo",
        ["doc", ...packageFlags, "--all-features", "--no-deps", "--locked"],
        {
          cwd: ROOT,
          env: { ...process.env, RUSTDOCFLAGS: "-D warnings" },
        },
      );
    }
  } else {
    const clippyGroups = [
      "all",
      "cargo",
      "complexity",
      "correctness",
      "nursery",
      "pedantic",
      "perf",
      "restriction",
      "style",
      "suspicious",
    ];
    const clippyArgs = [
      "clippy",
      ...packageFlags,
      "--all-targets",
      "--all-features",
      "--locked",
      "--",
      "-Dwarnings",
      ...clippyGroups.flatMap((group) => ["-D", `clippy::${group}`]),
      ...Object.keys(exceptions).flatMap((lint) => ["-A", `clippy::${lint}`]),
    ];
    run("cargo", clippyArgs, {
      cwd: ROOT,
      env: {
        ...process.env,
        RUSTFLAGS: "--cfg quickshare_oracle_reference",
      },
    });
  }
}
