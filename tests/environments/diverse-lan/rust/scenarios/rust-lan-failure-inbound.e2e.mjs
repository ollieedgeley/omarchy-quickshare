import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("Rust receiver fails when reference sender disappears", async () => {
  await runRustLanScenario({
    direction: "google-to-rust",
    outcome: "failed",
  });
});
