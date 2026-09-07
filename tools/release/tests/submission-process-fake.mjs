#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";

const POLL_MS = 10;
const args = process.argv.slice(2);

if (args[0] === "send") {
  writeFileSync(process.env.SUBMISSION_ATTEMPTS, "send\n", { flag: "a" });
}

function execute() {
  const result = spawnSync(process.env.QUICKSHARE_REAL_BINARY, args, {
    stdio: "inherit",
  });
  return result.status ?? 1;
}

function waitForRelease(complete) {
  writeFileSync(process.env.SUBMISSION_STARTED, "started");
  const timer = setInterval(() => {
    if (existsSync(process.env.SUBMISSION_RELEASE)) {
      clearInterval(timer);
      complete();
    }
  }, POLL_MS);
}

if (args[0] !== "send") {
  process.exitCode = execute();
} else if (process.env.SUBMISSION_MODE === "after") {
  const status = execute();
  waitForRelease(() => {
    process.exitCode = status;
  });
} else {
  waitForRelease(() => {
    process.exitCode = execute();
  });
}
