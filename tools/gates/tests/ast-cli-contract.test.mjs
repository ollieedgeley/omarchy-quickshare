import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const AST_GREP = join(ROOT, "node_modules", ".bin", "ast-grep");

function fixtureRoot(source) {
  const root = mkdtempSync(join(tmpdir(), "quickshare-ast-contract-"));
  mkdirSync(join(root, "rules"));
  writeFileSync(join(root, "sgconfig.yml"), "ruleDirs:\n  - rules\n");
  writeFileSync(
    join(root, "rules", "no-todo.yml"),
    [
      "id: no-todo",
      "language: Rust",
      "severity: error",
      "message: todo is forbidden",
      "files:",
      '  - "**/*.rs"',
      "rule:",
      "  pattern:",
      '    context: "todo!()"',
      "    strictness: cst",
      "",
    ].join("\n"),
  );
  writeFileSync(join(root, "case.rs"), source);
  return root;
}

function scanStatus(source) {
  const root = fixtureRoot(source);
  try {
    return spawnSync(
      AST_GREP,
      [
        "scan",
        "--config",
        "sgconfig.yml",
        "--error",
        "--min-severity=error",
        "--max-results=1",
        ".",
      ],
      { cwd: root, encoding: "utf8" },
    ).status;
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("strict scan fails a normal project-rule violation", () => {
  assert.notEqual(scanStatus("fn main() { todo!(); }\n"), 0);
});

test("strict scan fails a bare suppression", () => {
  assert.notEqual(
    scanStatus("fn main() {\n// ast-grep-ignore\nlet value = 1;\n}\n"),
    0,
  );
});

test("strict scan fails an unused named suppression", () => {
  assert.notEqual(
    scanStatus("fn main() {\n// ast-grep-ignore: no-todo\nlet value = 1;\n}\n"),
    0,
  );
});
