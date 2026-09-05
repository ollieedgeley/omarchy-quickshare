import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("Retry12 interleaves with file data in both Rust roles", async () => {
  await runRustLanScenario({ direction: "google-to-rust", retry: true });
  await runRustLanScenario({ direction: "rust-to-google", retry: true });
});
