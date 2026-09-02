import assert from "node:assert/strict";
import { test } from "node:test";

import { ESLint } from "eslint";
import { builtinRules } from "eslint/use-at-your-own-risk";

const ERROR_SEVERITY = 2;
const REPRESENTATIVE_FILE = "tools/gates/structure.mjs";

test("every current core JavaScript rule is an error", async () => {
  const eslint = new ESLint();
  const config = await eslint.calculateConfigForFile(REPRESENTATIVE_FILE);
  const expectedRules = [...builtinRules]
    .filter(([, rule]) => !rule.meta.deprecated)
    .map(([name]) => name)
    .sort();
  const missing = expectedRules.filter((name) => !config.rules[name]);
  const downgraded = expectedRules.filter(
    (name) => config.rules[name]?.[0] !== ERROR_SEVERITY,
  );

  assert.deepEqual(missing, []);
  assert.deepEqual(downgraded, []);
  assert.equal(config.linterOptions.noInlineConfig, true);
  assert.equal(
    config.linterOptions.reportUnusedDisableDirectives,
    ERROR_SEVERITY,
  );
  assert.equal(config.linterOptions.reportUnusedInlineConfigs, ERROR_SEVERITY);
});
