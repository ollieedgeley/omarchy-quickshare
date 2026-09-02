import { spawn } from "node:child_process";

const DEFAULT_STOP_MS = 2_000;
const DEFAULT_WAIT_MS = 20_000;
const AUTH_FIELD_PATTERN = /\b(?<name>auth_digits|token)=\S+/gu;
const CONFIRMATION_TOKEN_PATTERN = /confirmation token:\s*\S+/giu;
const FAILURE_SIGNAL_PATTERN =
  /QS_EVENT|error|fail|status|timeout|target|confirmation token/iu;
const SUMMARY_LINE_LIMIT = 16;

function delay(milliseconds) {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}

function composeArguments(compose, command) {
  const args = ["compose", "--file", compose, "exec", "--tty=false"];
  for (const [name, value] of Object.entries(command.variables ?? {})) {
    args.push("--env", `${name}=${value}`);
  }
  return [
    ...args,
    command.peer,
    "runuser",
    "--user",
    "quickshare",
    "--",
    ...command.args,
  ];
}

function capture(child) {
  let log = "";
  child.stdout.on("data", (chunk) => {
    log += chunk;
  });
  child.stderr.on("data", (chunk) => {
    log += chunk;
  });
  return () => log;
}

function completion(child) {
  return new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (code, signal) => {
      resolve({ code, signal });
    });
  });
}

export function failureSummary(log) {
  const lines = log
    .split("\n")
    .filter((line) => FAILURE_SIGNAL_PATTERN.test(line))
    .slice(-SUMMARY_LINE_LIMIT);
  return lines
    .join("\n")
    .replace(AUTH_FIELD_PATTERN, "$<name>=<redacted>")
    .replace(CONFIRMATION_TOKEN_PATTERN, "confirmation token: <redacted>");
}

async function stop(child, completed, stopMs) {
  if (child.exitCode !== null || child.signalCode !== null) {
    await completed;
    return;
  }
  child.kill("SIGINT");
  const stopped = await Promise.race([
    completed.then(() => true),
    delay(stopMs).then(() => false),
  ]);
  if (!stopped) {
    child.kill("SIGKILL");
    await completed;
  }
}

async function waitFor({ completed, logs, options, stopChild }) {
  const timeoutMs = options.timeoutMs ?? DEFAULT_WAIT_MS;
  const result = await Promise.race([
    completed,
    delay(timeoutMs).then(() => null),
  ]);
  if (!result) {
    await stopChild();
    const summary = failureSummary(logs());
    throw new Error(
      `Nearby Linux peer command exceeded its time limit\n${summary}`,
    );
  }
  const acceptedCodes = options.acceptedCodes ?? [0];
  if (!acceptedCodes.includes(result.code)) {
    const summary = failureSummary(logs());
    throw new Error(
      `Nearby Linux peer command exited with ` +
        `${result.code ?? result.signal}\n${summary}`,
    );
  }
  return result;
}

export function createComposeRunner(options) {
  return {
    start(command) {
      const child = spawn(
        options.docker,
        composeArguments(options.compose, command),
        { env: options.environment, stdio: ["ignore", "pipe", "pipe"] },
      );
      const logs = capture(child);
      const completed = completion(child);
      const stopChild = () =>
        stop(child, completed, options.stopMs ?? DEFAULT_STOP_MS);
      return {
        logs,
        stop: stopChild,
        wait: (waitOptions = {}) =>
          waitFor({
            completed,
            logs,
            options: waitOptions,
            stopChild,
          }),
      };
    },
  };
}

export { composeArguments };
