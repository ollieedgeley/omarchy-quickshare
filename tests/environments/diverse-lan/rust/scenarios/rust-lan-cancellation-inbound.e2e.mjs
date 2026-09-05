import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("reference sender cancels before Rust consent", async () => {
  await runRustLanScenario({
    direction: "google-to-rust",
    outcome: "cancelled",
  });
});
