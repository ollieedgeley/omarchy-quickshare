import assert from "node:assert/strict";
import test from "node:test";

import * as transfer from "./transfer.mjs";

test("Sharing cleanup failures fail the suite", () =>
  assert.rejects(
    transfer.stopPeers([
      { stop: () => Promise.reject(new Error("cleanup failed")) },
    ]),
  ));
