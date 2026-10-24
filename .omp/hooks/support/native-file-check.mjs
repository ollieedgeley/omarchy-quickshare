import { readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";

import { runAnalysis } from "../../../tools/gates/lib/analysis.mjs";
import { fail, output, run } from "../../../tools/gates/lib/process.mjs";
import { packageSelection } from "../../../tools/hooks/affected.mjs";

function checkRust(paths, cwd) {
  const metadata = JSON.parse(
    output("cargo", ["metadata", "--format-version", "1", "--locked"], { cwd }),
  );
  const unowned = paths.filter(
    (path) => packageSelection(metadata, [path], cwd).length === 0,
  );
  if (unowned.length) {
    throw new Error(
      `Strict Clippy: no workspace package owns ${unowned.join(", ")}`,
    );
  }
  const packages = packageSelection(metadata, paths, cwd);
  if (!packages.length) {
    throw new Error("Strict Clippy requires at least one owning package");
  }
  run(
    process.execPath,
    [
      join(cwd, "tools/gates/rust-lints.mjs"),
      ...packages.flatMap((pkg) => ["--package", pkg.name]),
    ],
    { cwd, env: { ...process.env, RUST_LINT_STEP: "clippy" } },
  );
}

function checkQml(file, cwd) {
  const result = run(
    process.env.QMLFORMAT ?? "/usr/lib/qt6/bin/qmlformat",
    ["--settings", join(cwd, ".qmlformat.ini"), resolve(cwd, file)],
    { capture: true, cwd, quiet: true },
  );
  if (result.stdout !== readFileSync(resolve(cwd, file), "utf8")) {
    throw new Error(
      `${file}: qmlformat formatting differs from .qmlformat.ini`,
    );
  }
}

try {
  const [mode, ...files] = process.argv.slice(2);
  const cwd = process.cwd();
  if (!files.length) {
    throw new Error("native-file-check.mjs requires explicit file paths");
  }
  const paths = files.map((file) => relative(cwd, resolve(cwd, file)));
  if (mode === "rust-clippy") {
    checkRust(paths, cwd);
  } else if (mode === "qmlformat-check" && paths.length === 1) {
    checkQml(paths[0], cwd);
  } else if (mode === "checkstyle" || mode === "lizard") {
    runAnalysis({ cwd, paths, requestedAnalyzers: [mode], scope: "files" });
  } else {
    throw new Error(`Unknown native file check: ${mode}`);
  }
} catch (error) {
  fail(error.message);
}
