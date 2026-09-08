import { defineConfig } from "vitest/config";

const TEST_TIMEOUT_MS = 30_000;

export default defineConfig({
  test: {
    environment: "node",
    fileParallelism: false,
    projects: [
      {
        test: {
          name: "tooling",
          include: ["tools/gates/tests/**/*.test.mjs"],
        },
      },
      {
        test: {
          name: "release",
          include: ["tools/release/tests/**/*.test.mjs"],
        },
      },
      {
        test: {
          name: "environments",
          include: ["tests/environments/**/*.test.mjs"],
        },
      },
    ],
    testTimeout: TEST_TIMEOUT_MS,
  },
});
