import { spawnSync } from "node:child_process";
import { closeSync, openSync } from "node:fs";

const PRIVATE_FILE_MODE = 0o600;

function spawn(command, args, options) {
  let standardIo = "inherit";
  if (options.capture) {
    standardIo = "pipe";
  }
  let log = null;
  try {
    if (options.logPath) {
      log = openSync(options.logPath, "wx", PRIVATE_FILE_MODE);
      let stdin = "inherit";
      if (Object.hasOwn(options, "input")) {
        stdin = "pipe";
      }
      standardIo = [stdin, log, log];
    }
    return spawnSync(command, args, {
      cwd: options.cwd,
      encoding: "utf8",
      env: options.env ?? process.env,
      input: options.input,
      stdio: standardIo,
    });
  } finally {
    if (log !== null) {
      closeSync(log);
    }
  }
}

export function run(command, args, options = {}) {
  const rendered = [command, ...args].join(" ");
  if (!options.quiet) {
    process.stdout.write(`+ ${rendered}\n`);
  }
  const result = spawn(command, args, options);
  if (result.error) {
    throw new Error(`${rendered}: ${result.error.message}`);
  }
  if (result.status !== 0 && !options.allowFailure) {
    let detail = "";
    if (options.capture) {
      detail = `\n${result.stdout ?? ""}${result.stderr ?? ""}`;
    }
    throw new Error(`${rendered} exited with ${result.status}${detail}`);
  }
  return result;
}

export function output(command, args, options = {}) {
  return run(command, args, {
    ...options,
    capture: true,
    quiet: true,
  }).stdout.trim();
}

export function withoutRepositoryGitEnvironment(env) {
  const childEnv = { ...env };
  const localNames = output("git", ["rev-parse", "--local-env-vars"]);
  for (const name of localNames.split("\n")) {
    delete childEnv[name];
  }
  return childEnv;
}

export function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exitCode = 1;
}
