import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("Rust receives reference text bytes", async () => {
  await runRustLanScenario({ direction: "google-to-rust", kind: "text" });
});

test("Rust receives reference URL bytes around Retry12", async () => {
  await runRustLanScenario({
    direction: "google-to-rust",
    kind: "url",
    retry: true,
  });
});
