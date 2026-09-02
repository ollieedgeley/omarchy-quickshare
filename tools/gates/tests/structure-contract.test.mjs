import assert from "node:assert/strict";
import test from "node:test";

import { lineLimit } from "../structure.mjs";

test("test trees and test support receive the 800-line budget", () => {
  assert.equal(lineLimit("tests/environments/oracle/environment.mjs"), 800);
  assert.equal(lineLimit("crates/protocol/tests/wire.rs"), 800);
  assert.equal(lineLimit("tools/gates/example.test.mjs"), 800);
});

test("production and agent files retain the 500-line budget", () => {
  assert.equal(lineLimit("crates/protocol/src/wire.rs"), 500);
  assert.equal(lineLimit("AGENTS.md"), 500);
});
