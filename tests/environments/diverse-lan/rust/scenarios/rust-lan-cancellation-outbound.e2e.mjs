import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("Rust sender cancels before reference consent", async () => {
  await runRustLanScenario({
    direction: "rust-to-google",
    outcome: "cancelled",
  });
});
