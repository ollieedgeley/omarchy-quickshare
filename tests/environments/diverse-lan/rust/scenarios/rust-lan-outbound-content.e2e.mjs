import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("reference peer receives Rust text bytes", async () => {
  await runRustLanScenario({ direction: "rust-to-google", kind: "text" });
});

test("reference peer receives Rust URL bytes around Retry12", async () => {
  await runRustLanScenario({
    direction: "rust-to-google",
    kind: "url",
    retry: true,
  });
});
