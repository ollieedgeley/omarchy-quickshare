import fs from "node:fs";
import path from "node:path";

const BOUND = 2048;
const TIMEOUT_MS = 60000;
const INTERNAL_URI_RE = /^[a-z][a-z0-9+.-]*:\/\//iu;

function bounded(input) {
  if (!input) {
    return "";
  }
  const str = String(input);
  if (str.length > BOUND) {
    return `${str.slice(0, BOUND)}\n... (truncated)`;
  }
  return str;
}

function extractCandidates(event) {
  const set = new Set();
  if (event.toolName === "write") {
    const pathCandidate =
      event.details?.resolvedPath || event.details?.path || event.input?.path;
    if (pathCandidate) {
      set.add(pathCandidate);
    }
  } else if (event.toolName === "edit") {
    if (event.details?.path) {
      set.add(event.details.path);
    }
    const perFileResults = event.details?.perFileResults || [];
    for (const perFileResult of perFileResults) {
      if (perFileResult?.path) {
        set.add(perFileResult.path);
      }
    }
  }
  return [...set];
}

function isInternalUri(candidatePath) {
  return (
    typeof candidatePath === "string" && INTERNAL_URI_RE.test(candidatePath)
  );
}

function resolveCandidate(candidate, base) {
  if (isInternalUri(candidate)) {
    return null;
  }
  let absolutePath = candidate;
  if (!path.isAbsolute(candidate)) {
    absolutePath = path.resolve(base, candidate);
  }
  const relativePath = path.relative(base, absolutePath);
  if (
    relativePath === ".." ||
    relativePath.startsWith("../") ||
    path.isAbsolute(relativePath) ||
    !fs.existsSync(absolutePath)
  ) {
    return null;
  }
  try {
    if (fs.statSync(absolutePath).isFile()) {
      return absolutePath;
    }
  } catch {
    return null;
  }
  return null;
}

function resolveValidTargets(candidates, cwd) {
  const base = path.resolve(cwd || process.cwd());
  const resolved = candidates.map((candidate) =>
    resolveCandidate(candidate, base),
  );
  return [...new Set(resolved.filter(Boolean))];
}

function commandsForFile(file, cwd) {
  const extension = path.extname(file).toLowerCase();
  const nodeBin = path.join(cwd, "node_modules/.bin");
  const prettier = path.join(nodeBin, "prettier");
  const eslint = path.join(nodeBin, "eslint");
  const markdownlint = path.join(nodeBin, "markdownlint-cli2");
  const ruff = path.join(cwd, ".cache/tools/ruff-0.16.5/ruff");

  if (extension === ".rs") {
    return [
      { command: "rustfmt", args: ["--edition", "2024", "--check", file] },
    ];
  }

  const isJavaScript = [".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"].includes(
    extension,
  );
  if (isJavaScript) {
    return [
      { command: prettier, args: ["--check", file] },
      {
        command: eslint,
        args: ["--max-warnings", "0", "--no-warn-ignored", file],
      },
    ];
  }

  if (extension === ".py" || extension === ".pyi") {
    return [
      { command: ruff, args: ["format", "--check", file] },
      { command: ruff, args: ["check", file] },
    ];
  }

  if (extension === ".md" || extension === ".markdown") {
    return [
      { command: prettier, args: ["--check", file] },
      { command: markdownlint, args: [file] },
    ];
  }

  const isData = [".json", ".jsonc", ".yaml", ".yml"].includes(extension);
  if (isData) {
    return [{ command: prettier, args: ["--check", file] }];
  }

  return [];
}

async function firstFailedCheck(pi, checks, state) {
  const { cwd, index = 0 } = state;
  if (index >= checks.length) {
    return null;
  }
  const { command, args } = checks.at(index);
  const result = await pi.exec(command, args, { cwd, timeout: TIMEOUT_MS });
  if (result.code !== 0) {
    return { command, result };
  }
  return firstFailedCheck(pi, checks, { cwd, index: index + 1 });
}

export async function handleToolResult(pi, event, ctx) {
  if (
    event.isError ||
    (event.toolName !== "write" && event.toolName !== "edit")
  ) {
    return Promise.resolve();
  }

  const candidates = extractCandidates(event);
  const cwd = ctx?.cwd || process.cwd();
  const targets = resolveValidTargets(candidates, cwd);
  const checks = targets.flatMap((file) => commandsForFile(file, cwd));
  const failure = await firstFailedCheck(pi, checks, { cwd });
  if (!failure) {
    return Promise.resolve();
  }
  const standardOutput = failure.result.stdout ?? "";
  const standardError = failure.result.stderr ?? "";
  const outputText = `${standardOutput}\n${standardError}`;
  const text = bounded(`Check failed (${failure.command}):\n${outputText}`);
  return {
    content: [...(event.content || []), { type: "text", text }],
    details: event.details,
    isError: true,
  };
}

export default function checkEditedFiles(pi) {
  pi.on("tool_result", (event, ctx) => handleToolResult(pi, event, ctx));
}
