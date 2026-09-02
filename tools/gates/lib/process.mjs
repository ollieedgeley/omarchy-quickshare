import { spawnSync } from "node:child_process";

export function run(command, args, options = {}) {
  const rendered = [command, ...args].join(" ");
  if (!options.quiet) {
    process.stdout.write(`+ ${rendered}\n`);
  }

  const result = spawnSync(command, args, {
    cwd: options.cwd,
    encoding: "utf8",
    env: options.env ?? process.env,
    input: options.input,
    stdio: options.capture ? "pipe" : "inherit",
  });

  if (result.error) {
    throw new Error(`${rendered}: ${result.error.message}`);
  }
  if (result.status !== 0 && !options.allowFailure) {
    const detail = options.capture
      ? `\n${result.stdout ?? ""}${result.stderr ?? ""}`
      : "";
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

export function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exitCode = 1;
}
