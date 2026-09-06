import { createHash } from "node:crypto";
import {
  lstatSync,
  mkdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
} from "node:fs";
import { join, resolve } from "node:path";

import {
  output,
  run,
  withoutRepositoryGitEnvironment,
} from "../../gates/lib/process.mjs";
import { runRecorded, updateReport } from "../records.mjs";

const OCTAL_RADIX = 8;
const PERMISSION_DIGITS = -3;
const EXECUTABLE_PERMISSION = /[1357]/u;

function blobBytes(file, stat) {
  if (stat.isSymbolicLink()) {
    return Buffer.from(readlinkSync(file));
  }
  return readFileSync(file);
}

function blobMode(stat) {
  if (stat.isSymbolicLink()) {
    return "120000";
  }
  if (!stat.isFile()) {
    throw new Error("staged blob was replaced with a non-file");
  }
  if (
    EXECUTABLE_PERMISSION.test(
      stat.mode.toString(OCTAL_RADIX).slice(PERMISSION_DIGITS),
    )
  ) {
    return "100755";
  }
  return "100644";
}

export function verifySnapshot(staged) {
  const tree = output("git", ["write-tree"], { cwd: staged.root });
  if (tree !== staged.tree) {
    throw new Error("the Git index changed; rerun `make pre-commit-prepare`");
  }
  const entries = run("git", ["ls-tree", "-r", "-z", staged.tree], {
    cwd: staged.root,
    capture: true,
    quiet: true,
  })
    .stdout.split("\0")
    .filter(Boolean);
  for (const entry of entries) {
    const separator = entry.indexOf("\t");
    const path = entry.slice(separator + 1);
    const [mode, type, hash] = entry.slice(0, separator).split(" ");
    if (type !== "blob") {
      throw new Error(`unsupported staged snapshot entry: ${path}`);
    }
    const file = join(staged.mirror, path);
    const stat = lstatSync(file, { throwIfNoEntry: false });
    if (!stat || blobMode(stat) !== mode) {
      throw new Error(`staged mirror file mode changed or missing: ${path}`);
    }
    const bytes = blobBytes(file, stat);
    const actual = createHash("sha1")
      .update(`blob ${bytes.length}\0`)
      .update(bytes)
      .digest("hex");
    if (actual !== hash) {
      throw new Error(
        `staged mirror content changed: ${path}; rerun make pre-commit-prepare`,
      );
    }
  }
}

export function executionEnvironment(staged, tools) {
  const cache = resolve(
    process.env.TEST_ENV_CACHE ?? join(staged.root, ".cache/test-env"),
  );
  mkdirSync(cache, { recursive: true });
  const env = {
    ...withoutRepositoryGitEnvironment(process.env),
    ...tools,
    CARGO_MACHETE: join(
      staged.root,
      ".cache/tools/cargo-machete-0.9.2/bin/cargo-machete",
    ),
    CARGO_TARGET_DIR: join(staged.root, "target"),
    ESLINT: join(tools.NODE_BIN, "eslint"),
    GATE_ROOT: staged.mirror,
    MARKDOWNLINT: join(tools.NODE_BIN, "markdownlint-cli2"),
    PATH: `${tools.NODE_BIN}:${process.env.PATH}`,
    PRETTIER: join(tools.NODE_BIN, "prettier"),
    TEST_ENV_CACHE: realpathSync(cache),
    VULTURE: join(staged.root, ".cache/tools/vulture-2.16/bin/vulture"),
  };
  delete env.MAKEFLAGS;
  delete env.MFLAGS;
  delete env.MAKEOVERRIDES;
  return env;
}

export function recordedStep(staged, step) {
  return runRecorded({
    reportPath: staged.reportPath,
    cwd: staged.mirror,
    ...step,
  });
}

export function cachedSelection(staged) {
  return JSON.parse(readFileSync(staged.reportPath, "utf8")).selection;
}

export function runIntegrations(staged, gates, env) {
  const prepared = new Set();
  const cleanup = new Set();
  const reasons = gates.flatMap((gate) => gate.reasons);
  const execute = (target, kind, why = reasons) => {
    verifySnapshot(staged);
    recordedStep(staged, {
      args: [target],
      command: "make",
      env,
      id: target,
      kind,
      reasons: why,
    });
  };
  let failure = null;
  try {
    for (const gate of gates) {
      gate.cleanup.forEach((target) => cleanup.add(target));
      for (const target of gate.prepare) {
        if (!prepared.has(target)) {
          execute(target, "prepare", gate.reasons);
          prepared.add(target);
        }
      }
      execute(gate.target, "integration", gate.reasons);
    }
  } catch (error) {
    failure = error;
  } finally {
    for (const target of [...cleanup].reverse()) {
      try {
        recordedStep(staged, {
          args: [target],
          command: "make",
          env,
          id: target,
          kind: "cleanup",
          reasons,
        });
      } catch (error) {
        failure ??= error;
      }
    }
  }
  if (failure) {
    throw failure;
  }
  updateReport(staged.reportPath, { status: "passed" });
}
