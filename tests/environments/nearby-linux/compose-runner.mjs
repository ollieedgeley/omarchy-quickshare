import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";

import * as failure from "../../../tools/gates/lib/failure-artifact.mjs";

const DEFAULT_STOP_MS = 2_000;
const DEFAULT_WAIT_MS = 20_000;
const STOP_POLL_MS = 25;
const EVENT_PATTERN = /\bevent=(?<event>\S+)/u;
const SAFE_EVENT_PATTERN = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const SAFE_VALUE_PATTERN = /[^a-z0-9-]/gu;
const STATUS_PATTERN = /\bstatus=(?<status>\S+)/u;
const SAFE_STATUSES = new Map([
  ["kAwaitingLocalConfirmation", "awaiting-local-confirmation"],
  ["kAwaitingRemoteAcceptance", "awaiting-remote-acceptance"],
  ["kCancelled", "cancelled"],
  ["kComplete", "completed"],
  ["kConnecting", "connecting"],
  ["kFailed", "failed"],
  ["kInProgress", "in-progress"],
  ["kOk", "succeeded"],
  ["kReject", "rejected"],
  ["kTimedOut", "timeout"],
]);
const COMMAND_DIRECTORY = "/run/quickshare/commands";
const START_SCRIPT = [
  "marker=$1",
  "shift",
  "umask 077",
  "mkdir -p /run/quickshare/commands",
  'printf "%s\\n" "$$" > "$marker"',
  "umask 022",
  'cleanup() { rm -f "$marker"; }',
  "trap cleanup EXIT",
  "trap ':' INT TERM",
  '"$@" &',
  "child=$!",
  "status=0",
  'while kill -0 "$child" 2>/dev/null; do',
  '  if wait "$child"; then status=0; else status=$?; fi',
  "done",
  'exit "$status"',
].join("\n");
const CONTROL_SCRIPT = [
  "action=$1",
  "marker=$2",
  "signal=$3",
  'test -r "$marker" || exit 1',
  'pid=$(cat "$marker")',
  'case "$action" in',
  "  exists) : ;;",
  '  signal) kill -"$signal" -- "-$pid" 2>/dev/null ;;',
  '  probe) kill -0 -- "-$pid" 2>/dev/null ;;',
  '  verify) ! pgrep --pgroup "$pid" >/dev/null 2>&1 ;;',
  '  remove) rm -f "$marker" ;;',
  "esac",
].join("\n");

function delay(milliseconds) {
  return new Promise((resolve) => {
    setTimeout(resolve, milliseconds);
  });
}

function markerPath() {
  return `${COMMAND_DIRECTORY}/${randomUUID()}.pid`;
}

function composeArguments(compose, command, marker) {
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
    "setsid",
    "sh",
    "-ceu",
    START_SCRIPT,
    "compose-runner",
    marker ?? markerPath(),
    ...command.args,
  ];
}

function controlArguments(compose, input) {
  return [
    "compose",
    "--file",
    compose,
    "exec",
    "--tty=false",
    input.peer,
    "runuser",
    "--user",
    "quickshare",
    "--",
    "sh",
    "-ceu",
    CONTROL_SCRIPT,
    "compose-runner",
    input.action,
    input.marker,
    input.signal,
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

function safeEvent(value) {
  return value
    .toLowerCase()
    .replaceAll("_", "-")
    .replace(SAFE_VALUE_PATTERN, "");
}

function safeStatus(value) {
  const known = SAFE_STATUSES.get(value);
  if (known) {
    return known;
  }
  const lower = value.toLowerCase();
  if (lower.includes("timeout")) {
    return "timeout";
  }
  if (lower.includes("fail") || lower.includes("error")) {
    return "failed";
  }
  return "unknown";
}

export function failureEvents(log) {
  return log
    .split("\n")
    .filter((line) => line.startsWith("QS_EVENT"))
    .flatMap((line) => {
      const event = EVENT_PATTERN.exec(line)?.groups?.event;
      if (!event) {
        return [];
      }
      const safe = safeEvent(event);
      if (!SAFE_EVENT_PATTERN.test(safe)) {
        return [];
      }
      return [
        {
          event: safe,
          status: safeStatus(STATUS_PATTERN.exec(line)?.groups?.status ?? ""),
        },
      ];
    });
}

function recordFailure(options, outcome) {
  if (!options.failureDirectory) {
    return;
  }
  failure.writeFailureArtifact(options.failureDirectory, {
    events: failureEvents(options.logs()),
    gate: "nearby-linux",
    outcome,
    stage: "peer-command",
  });
}

async function stopClient(child, completed, stopMs) {
  if (child.exitCode !== null || child.signalCode !== null) {
    await completed;
    return;
  }
  child.kill("SIGINT");
  const clientStopped = await Promise.race([
    completed.then(() => true),
    delay(stopMs).then(() => false),
  ]);
  if (!clientStopped) {
    child.kill("SIGKILL");
    await completed;
  }
}

function composeChild(options, args, stdio) {
  return (options.spawn ?? spawn)(options.docker, args, {
    env: options.environment,
    stdio,
  });
}

async function control(options, input) {
  const child = composeChild(
    options,
    controlArguments(options.compose, input),
    "ignore",
  );
  const result = await completion(child);
  return result.code === 0;
}

function markerExists(options, input) {
  return control(options, { ...input, action: "exists" });
}

async function waitForMarker(options, input) {
  if (await markerExists(options, input)) {
    return true;
  }
  const ended = await Promise.race([
    input.completed.then(() => true),
    delay(STOP_POLL_MS).then(() => false),
  ]);
  if (ended || Date.now() >= input.deadline) {
    return false;
  }
  return waitForMarker(options, input);
}

async function findLateMarker(options, input, deadline) {
  if (await markerExists(options, input)) {
    return true;
  }
  if (Date.now() >= deadline) {
    return false;
  }
  await delay(STOP_POLL_MS);
  return findLateMarker(options, input, deadline);
}

async function waitForProcessStop(options, input, deadline) {
  if (!(await control(options, { ...input, action: "probe" }))) {
    return;
  }
  if (Date.now() >= deadline) {
    await control(options, { ...input, action: "signal", signal: "KILL" });
    return;
  }
  await delay(STOP_POLL_MS);
  await waitForProcessStop(options, input, deadline);
}

async function stopOwnedProcess(options, input) {
  if (
    !(await control(options, { ...input, action: "signal", signal: "INT" }))
  ) {
    return;
  }
  await waitForProcessStop(options, input, Date.now() + input.stopMs);
  if (await control(options, { ...input, action: "probe" })) {
    throw new Error("Nearby Linux peer command did not stop");
  }
  if (!(await control(options, { ...input, action: "verify" }))) {
    throw new Error("Nearby Linux peer command left a stale process group");
  }
  await control(options, { ...input, action: "remove" });
}

async function waitFor({ completed, logs, options, stopChild }) {
  const timeoutMs = options.timeoutMs ?? DEFAULT_WAIT_MS;
  const result = await Promise.race([
    completed,
    delay(timeoutMs).then(() => null),
  ]);
  if (!result) {
    await stopChild();
    recordFailure({ ...options, logs }, { kind: "timeout" });
    throw new Error("Nearby Linux peer command exceeded its time limit");
  }
  const acceptedCodes = options.acceptedCodes ?? [0];
  if (!acceptedCodes.includes(result.code)) {
    const code = result.code ?? 0;
    recordFailure({ ...options, logs }, { code, kind: "exit" });
    throw new Error(`Nearby Linux peer command exited with ${code}`);
  }
  return result;
}

function startPeerProcess(options, command) {
  const marker = markerPath();
  const child = composeChild(
    options,
    composeArguments(options.compose, command, marker),
    ["ignore", "pipe", "pipe"],
  );
  const logs = capture(child);
  const completed = completion(child);
  const stopChild = async () => {
    const stopMs = options.stopMs ?? DEFAULT_STOP_MS;
    const input = {
      completed,
      deadline: Date.now() + stopMs,
      marker,
      peer: command.peer,
      signal: "",
      stopMs,
    };
    const markerReady = await waitForMarker(options, input);
    if (markerReady) {
      await stopOwnedProcess(options, input);
    }
    await stopClient(child, completed, stopMs);
    if (
      !markerReady &&
      (await findLateMarker(options, input, Date.now() + stopMs))
    ) {
      await stopOwnedProcess(options, input);
    }
  };
  return {
    logs,
    stop: stopChild,
    wait: (waitOptions = {}) =>
      waitFor({
        completed,
        logs,
        options: { ...waitOptions, failureDirectory: options.failureDirectory },
        stopChild,
      }),
  };
}

export function createComposeRunner(options) {
  return { start: (command) => startPeerProcess(options, command) };
}

export { composeArguments };
