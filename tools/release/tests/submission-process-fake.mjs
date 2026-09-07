#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";

const POLL_MS = 10;
const args = process.argv.slice(2);
const journey = JSON.parse(process.env.CAPTURE_JOURNEY);

if (args[0] === "send") {
  writeFileSync(process.env.SUBMISSION_ATTEMPTS, "send\n", { flag: "a" });
}

function execute() {
  const result = spawnSync(process.env.QUICKSHARE_REAL_BINARY, args, {
    stdio: "inherit",
  });
  return result.status ?? 1;
}

function projectStatus() {
  const result = spawnSync(process.env.QUICKSHARE_REAL_BINARY, args, {
    encoding: "utf8",
  });
  process.stderr.write(result.stderr ?? "");
  if (result.status !== 0) {
    process.stdout.write(result.stdout ?? "");
    return result.status ?? 1;
  }
  const response = JSON.parse(result.stdout);
  const { snapshot } = response.response;
  if (journey.peerProjection === "reorder") {
    snapshot.peers.reverse();
  } else {
    const original = snapshot.peers.find((peer) => peer.id === "pixel-8");
    snapshot.peers = snapshot.peers.filter((peer) => peer.id !== "pixel-8");
    if (journey.peerProjection === "replace" && original) {
      snapshot.peers.unshift({ ...original, id: "replacement-for-pixel-8" });
    }
  }
  process.stdout.write(JSON.stringify(response));
  return 0;
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

let projectionActive = existsSync(process.env.PROJECTION_ACTIVE);
if (journey.peerProjection === "appear") {
  projectionActive = !projectionActive;
}

if (args[0] === "status" && journey.peerProjection && projectionActive) {
  process.exitCode = projectStatus();
} else if (args[0] !== "send" || !process.env.SUBMISSION_MODE) {
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
