#!/usr/bin/env node
import {
  appendFileSync,
  existsSync,
  readFileSync,
  writeFileSync,
} from "node:fs";

const POLL_MS = 10;
const READ_FAILURE_EXIT_CODE = 7;
const log = process.env.CLIPBOARD_LOG;
const previousReads = readFileSync(log, "utf8");
appendFileSync(log, "read\n");
const journey = JSON.parse(process.env.CAPTURE_JOURNEY);

function completeRead() {
  const failure = journey.explicitFailure || journey.invalidRead;
  if (failure === "empty") {
    process.exitCode = 0;
    return;
  }
  if (failure === "unsupported") {
    const args = process.argv.slice(2);
    const index = args.findIndex((arg) => arg === "--type" || arg === "-t");
    let requested = "";
    if (index >= 0) {
      requested = args[index + 1];
    }
    if (
      requested === "" ||
      requested === "image" ||
      requested === "image/png"
    ) {
      process.stdout.write("unsupported image bytes");
    } else {
      process.exitCode = 1;
    }
    return;
  }
  if (failure === "failed") {
    process.exitCode = READ_FAILURE_EXIT_CODE;
  } else if (process.env.CLIPBOARD_FAILURE === "true") {
    process.exitCode = 1;
  } else {
    process.stdout.write(process.env.CLIPBOARD_VALUE);
  }
}

if (process.env.CLIPBOARD_REPLACEMENT === "true" && previousReads === "") {
  writeFileSync(process.env.CLIPBOARD_STARTED, "started");
  const timer = setInterval(() => {
    if (existsSync(process.env.CLIPBOARD_RELEASE)) {
      clearInterval(timer);
      if (journey.explicitFailure) {
        completeRead();
      } else if (journey.explicitPending || journey.peerProjection) {
        process.stdout.write(journey.value);
      } else {
        process.stdout.write("superseded automatic B");
      }
    }
  }, POLL_MS);
} else {
  completeRead();
}
