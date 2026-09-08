import { defineConfig } from "vitest/config";

const PROCESS_TIMEOUT_MS = 300_000;

export default defineConfig({
  test: {
    environment: "node",
    fileParallelism: false,
    include: [
      "tools/release/tests/*.e2e.mjs",
      "tests/environments/**/*.e2e.mjs",
      "tests/scenarios/**/*.e2e.mjs",
    ],
    testTimeout: PROCESS_TIMEOUT_MS,
  },
});
