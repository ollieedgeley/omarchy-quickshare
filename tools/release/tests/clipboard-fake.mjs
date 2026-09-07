#!/usr/bin/env node
import {
  appendFileSync,
  existsSync,
  readFileSync,
  writeFileSync,
} from "node:fs";

const POLL_MS = 10;
const log = process.env.CLIPBOARD_LOG;
const previousReads = readFileSync(log, "utf8");
appendFileSync(log, "read\n");
if (process.env.CLIPBOARD_REPLACEMENT === "true" && previousReads === "") {
  writeFileSync(process.env.CLIPBOARD_STARTED, "started");
  const timer = setInterval(() => {
    if (existsSync(process.env.CLIPBOARD_RELEASE)) {
      clearInterval(timer);
      process.stdout.write("superseded automatic B");
    }
  }, POLL_MS);
} else if (process.env.CLIPBOARD_FAILURE === "true") {
  process.exitCode = 1;
} else {
  process.stdout.write(process.env.CLIPBOARD_VALUE);
}
