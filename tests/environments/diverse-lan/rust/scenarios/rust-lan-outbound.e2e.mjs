import { test } from "vitest";

import { runRustLanScenario } from "../rust-lan.mjs";

test("the Rust daemon sends to the reference LAN peer", async () => {
  await runRustLanScenario({ direction: "rust-to-google" });
});
