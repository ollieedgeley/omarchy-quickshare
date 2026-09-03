import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "./lib/process.mjs";

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

const rustcVersion = output("rustc", ["--version"], { cwd: ROOT });
const analyzerVersion = output("rust-analyzer", ["--version"], { cwd: ROOT });
if (!rustcVersion.includes("1.98.0")) {
  throw new Error(`expected Rust 1.98.0, received ${rustcVersion}`);
}
if (!analyzerVersion.includes("1.98.0")) {
  throw new Error(`expected rust-analyzer 1.98.0, received ${analyzerVersion}`);
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
    `unaccounted rustc allow-by-default lints: ${missingRustcLints.join(", ")}`,
  );
}

run(
  "cargo",
  ["check", "--workspace", "--all-targets", "--all-features", "--locked"],
  { cwd: ROOT },
);
run("rust-analyzer", ["diagnostics", ".", "--severity", "warning"], {
  cwd: ROOT,
});

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
  "--workspace",
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

run(
  "cargo",
  ["doc", "--workspace", "--all-features", "--no-deps", "--locked"],
  {
    cwd: ROOT,
    env: { ...process.env, RUSTDOCFLAGS: "-D warnings" },
  },
);
