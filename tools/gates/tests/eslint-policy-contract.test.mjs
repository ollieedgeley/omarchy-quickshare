import assert from "node:assert/strict";
import { test } from "node:test";

import { ESLint } from "eslint";
import { builtinRules } from "eslint/use-at-your-own-risk";
import regexp from "eslint-plugin-regexp";

const ERROR_SEVERITY = 2;
const REPRESENTATIVE_FILE = "tools/gates/structure.mjs";
const REGEX_NAMING_RULE = "no-restricted-syntax";

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

test("every recommended regexp plugin rule is an error", async () => {
  const eslint = new ESLint();
  const config = await eslint.calculateConfigForFile(REPRESENTATIVE_FILE);
  const expectedRules = Object.entries(regexp.configs["flat/recommended"].rules)
    .filter(
      ([name, severity]) => name.startsWith("regexp/") && severity !== "off",
    )
    .map(([name]) => name)
    .sort();
  const missing = expectedRules.filter((name) => !config.rules[name]);
  const downgraded = expectedRules.filter(
    (name) => config.rules[name]?.[0] !== ERROR_SEVERITY,
  );
  assert.deepEqual(missing, []);
  assert.deepEqual(downgraded, []);
});

test("regular expressions must initialize named variables", async () => {
  const eslint = new ESLint();
  const inline = await eslint.lintText("value.match(/inline/u);\n", {
    filePath: "fixture.mjs",
  });
  const named = await eslint.lintText(
    "const VALUE_PATTERN = /named/u;\nvalue.match(VALUE_PATTERN);\n",
    { filePath: "fixture.mjs" },
  );
  const inlineMessages = inline[0].messages.filter(
    ({ ruleId }) => ruleId === REGEX_NAMING_RULE,
  );
  const namedMessages = named[0].messages.filter(
    ({ ruleId }) => ruleId === REGEX_NAMING_RULE,
  );
  assert.equal(inlineMessages.length, 1);
  assert.deepEqual(namedMessages, []);
});
