import path from "node:path";

const JS_EXTENSIONS = new Set([".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"]);
const DATA_EXTENSIONS = new Set([".json", ".jsonc", ".yaml", ".yml"]);
const RUST_CHECK_TIMEOUT_MS = 600_000;

function commandPaths(cwd) {
  const nodeBin = path.join(cwd, "node_modules/.bin");
  return {
    eslint: path.join(nodeBin, "eslint"),
    markdownlint: path.join(nodeBin, "markdownlint-cli2"),
    nativeCheck: path.join(cwd, ".omp/hooks/support/native-file-check.mjs"),
    prettier: path.join(nodeBin, "prettier"),
    qmlformat: process.env.QMLFORMAT ?? "/usr/lib/qt6/bin/qmlformat",
    ruff: path.join(cwd, ".cache/tools/ruff-0.16.5/ruff"),
  };
}

function tool(command, kind, args) {
  return { args, command, kind, name: path.basename(command) };
}

function nativeCheck(commands, mode, files) {
  return {
    ...tool("node", "check", [commands.nativeCheck, mode, ...files]),
    name: mode,
  };
}

function rustPipeline(file, commands) {
  const args = ["--edition", "2024", "--config", "skip_children=true", file];
  return {
    checks: [
      tool("rustfmt", "format", [...args.slice(0, -1), "--check", file]),
      nativeCheck(commands, "checkstyle", [file]),
    ],
    fixes: [tool("rustfmt", "format", args)],
    language: "rust",
  };
}

function javascriptPipeline(file, commands, extension) {
  let language = "javascript";
  if (extension === ".ts" || extension === ".tsx") {
    language = "typescript";
  }
  const lintArgs = ["--max-warnings", "0", "--no-warn-ignored", file];
  return {
    checks: [
      tool(commands.prettier, "format", ["--check", file]),
      tool(commands.eslint, "lint", ["--format", "json", ...lintArgs]),
    ],
    fixes: [
      tool(commands.eslint, "lint", ["--fix", ...lintArgs]),
      tool(commands.prettier, "format", ["--write", file]),
    ],
    language,
  };
}

function pythonPipeline(file, commands) {
  const lintArgs = ["check", "--no-unsafe-fixes"];
  const styleChecks = [nativeCheck(commands, "lizard", [file])];
  if (file.endsWith(".py")) {
    styleChecks.push(nativeCheck(commands, "checkstyle", [file]));
  }
  return {
    checks: [
      tool(commands.ruff, "format", ["format", "--check", file]),
      tool(commands.ruff, "lint", [
        ...lintArgs,
        "--no-fix",
        "--output-format=json",
        file,
      ]),
      ...styleChecks,
    ],
    fixes: [
      tool(commands.ruff, "lint", [...lintArgs, "--fix", file]),
      tool(commands.ruff, "format", ["format", file]),
    ],
    language: "python",
  };
}

function qmlPipeline(file, commands, cwd) {
  return {
    checks: [
      nativeCheck(commands, "qmlformat-check", [file]),
      nativeCheck(commands, "checkstyle", [file]),
    ],
    fixes: [
      tool(commands.qmlformat, "format", [
        "--settings",
        path.join(cwd, ".qmlformat.ini"),
        "--inplace",
        file,
      ]),
    ],
    language: "qml",
  };
}

function markdownPipeline(file, commands) {
  return {
    checks: [
      tool(commands.prettier, "format", ["--check", file]),
      tool(commands.markdownlint, "lint", [file]),
    ],
    fixes: [
      tool(commands.markdownlint, "lint", ["--fix", file]),
      tool(commands.prettier, "format", ["--write", file]),
    ],
    language: "markdown",
  };
}

function dataPipeline(file, commands, extension) {
  let language = "yaml";
  if (extension.startsWith(".json")) {
    language = "json";
  }
  return {
    checks: [tool(commands.prettier, "format", ["--check", file])],
    fixes: [tool(commands.prettier, "format", ["--write", file])],
    language,
  };
}

export function pipelineForFile(file, cwd) {
  const extension = path.extname(file).toLowerCase();
  const commands = commandPaths(cwd);
  if (extension === ".rs") {
    return rustPipeline(file, commands);
  }
  if (JS_EXTENSIONS.has(extension)) {
    return javascriptPipeline(file, commands, extension);
  }
  if (extension === ".py" || extension === ".pyi") {
    return pythonPipeline(file, commands);
  }
  if (extension === ".qml") {
    return qmlPipeline(file, commands, cwd);
  }
  if (extension === ".md" || extension === ".markdown") {
    return markdownPipeline(file, commands);
  }
  if (DATA_EXTENSIONS.has(extension)) {
    return dataPipeline(file, commands, extension);
  }
  return null;
}

export function batchChecksForFiles(files, cwd) {
  const rustFiles = [...new Set(files)].filter(
    (file) => path.extname(file).toLowerCase() === ".rs",
  );
  if (!rustFiles.length) {
    return [];
  }
  return [
    {
      ...nativeCheck(commandPaths(cwd), "rust-clippy", rustFiles),
      timeout: RUST_CHECK_TIMEOUT_MS,
    },
  ];
}
