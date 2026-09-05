import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("the Rust daemon sends to the reference LAN peer", async () => {
  await runRustLanScenario({ direction: "rust-to-google" });
});
