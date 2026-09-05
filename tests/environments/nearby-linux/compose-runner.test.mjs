import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import test from "node:test";

const WAIT_TIMEOUT_MS = 1_500;
const MAX_REMAINING_HOLD_MS = 750;
const CHILD_TIMEOUT_MS = 3_000;

test("Compose runner releases its timeout after an immediate exit", () => {
  const runnerUrl = new URL("./compose-runner.mjs", import.meta.url).href;
  const script = `
    import { EventEmitter } from "node:events";
    import { createComposeRunner } from ${JSON.stringify(runnerUrl)};

    const runner = createComposeRunner({
      compose: "compose.yaml",
      docker: "docker",
      environment: {},
      spawn: () => {
        const child = new EventEmitter();
        child.stdout = new EventEmitter();
        child.stderr = new EventEmitter();
        child.exitCode = null;
        child.signalCode = null;
        queueMicrotask(() => {
          child.exitCode = 0;
          child.emit("close", 0, null);
        });
        return child;
      },
    });
    await runner.start({
      args: ["/usr/local/bin/file_share", "--help"],
      peer: "peer-a",
    }).wait({ timeoutMs: ${WAIT_TIMEOUT_MS} });
    const completedAt = performance.now();
    process.once("beforeExit", () => {
      console.log(performance.now() - completedAt);
    });
  `;
  const output = execFileSync(
    process.execPath,
    ["--input-type=module", "--eval", script],
    { encoding: "utf8", timeout: CHILD_TIMEOUT_MS },
  );
  const remainingHoldMs = Number(output.trim());
  assert.ok(
    remainingHoldMs < MAX_REMAINING_HOLD_MS,
    `completed wait kept the process alive for ${remainingHoldMs}ms`,
  );
});
