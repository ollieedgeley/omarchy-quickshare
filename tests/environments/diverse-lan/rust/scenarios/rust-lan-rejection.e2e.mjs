import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("each receiver can reject before bytes are persisted", async () => {
  await runRustLanScenario({
    direction: "google-to-rust",
    outcome: "rejected",
  });
  await runRustLanScenario({
    direction: "rust-to-google",
    outcome: "rejected",
  });
});
