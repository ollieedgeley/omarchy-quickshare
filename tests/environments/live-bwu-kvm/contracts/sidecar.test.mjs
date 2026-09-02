import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(
  new URL("../environment.mjs", import.meta.url),
  "utf8",
);
const UNCONFINED = /"--security-opt",\s+"seccomp=unconfined"/u;
const UNCONFINED_ALL = /seccomp=unconfined/gu;

test("only the btvirt sidecar relaxes Docker seccomp", () => {
  assert.match(source, UNCONFINED);
  assert.equal(source.match(UNCONFINED_ALL)?.length, 1);
});
