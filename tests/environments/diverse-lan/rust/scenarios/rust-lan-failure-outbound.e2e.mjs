import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("Rust sender fails when reference receiver disappears", async () => {
  await runRustLanScenario({
    direction: "rust-to-google",
    outcome: "failed",
  });
});
