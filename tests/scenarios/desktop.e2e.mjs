import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { test } from "vitest";

const DESKTOP = new URL("../environments/desktop/", import.meta.url);
const INSTALLATION = process.env.DESKTOP_SCENARIO_INSTALLATION;
let entry = "scenario.py";
let title = "retained desktop CLI transfers exact bytes to the reference peer";
if (INSTALLATION) {
  entry = "ui_scenario.py";
  title =
    "clean installed panel transfers exact bytes through visible controls";
}

test(title, async ({ onTestFinished }) => {
  assert.ok(process.env.DESKTOP_LAB, "Select an already-ready DESKTOP_LAB");
  const args = [fileURLToPath(new URL(entry, DESKTOP))];
  if (INSTALLATION) {
    args.push("--installation", INSTALLATION);
  }
  // Python owns admission, exclusive reset, public proof and retained evidence.
  const child = spawn(process.env.DESKTOP_SCENARIO_PYTHON ?? "python3", args, {
    stdio: "inherit",
  });
  const closed = new Promise((resolve) => {
    child.once("close", (code, signal) => resolve({ code, signal }));
  });
  const failed = new Promise((resolve, reject) => {
    child.once("error", reject);
  });
  onTestFinished(async () => {
    if (child.exitCode === null && child.signalCode === null) {
      // Interrupt only our scenario so Python can restore its scoped fault.
      child.kill("SIGINT");
    }
    await closed;
  });
  const { code, signal } = await Promise.race([closed, failed]);
  assert.equal(
    code,
    0,
    `${entry} failed (${signal ?? code}); see retained Python evidence`,
  );
});
