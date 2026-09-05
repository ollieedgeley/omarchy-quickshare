import test from "node:test";

import { runRustLanScenario } from "../rust-lan.mjs";

test("the Rust daemon receives from the reference LAN peer", async () => {
  await runRustLanScenario({ direction: "google-to-rust" });
});
