import js from "@eslint/js";
import stylistic from "@stylistic/eslint-plugin";
import { defineConfig, globalIgnores } from "eslint/config";
import regexp from "eslint-plugin-regexp";
import globals from "globals";

const APPLICATION_FILE_LIMIT = 500;
const FUNCTION_LINE_LIMIT = 50;
const LARGE_OBJECT_MINIMUM_KEYS = 4;
const MAXIMUM_LINE_LENGTH = 80;
const MAXIMUM_STATEMENTS = 40;
const TEST_FILE_LIMIT = 800;
const TEST_MAXIMUM_STATEMENTS = 60;
const regexpRules = Object.fromEntries(
  Object.entries(regexp.configs["flat/recommended"].rules)
    .filter(([, severity]) => severity !== "off")
    .map(([name]) => [name, "error"]),
);
const regexNamingRestrictions = [
  {
    message: "Assign this regular expression to a named variable.",
    selector: "Literal[regex]:not(VariableDeclarator > Literal[regex])",
  },
  {
    message: "Assign this RegExp construction to a named variable.",
    selector:
      "CallExpression[callee.name='RegExp']:not(" +
      "VariableDeclarator > CallExpression[callee.name='RegExp'])",
  },
  {
    message: "Assign this RegExp construction to a named variable.",
    selector:
      "NewExpression[callee.name='RegExp']:not(" +
      "VariableDeclarator > NewExpression[callee.name='RegExp'])",
  },
];
const javascriptFiles = ["**/*.js", "**/*.mjs", "**/*.cjs"];
const testFiles = [
  "tests/**/*.js",
  "tests/**/*.mjs",
  "tests/**/*.cjs",
  "tools/gates/tests/**/*.js",
  "tools/gates/tests/**/*.mjs",
  "tools/gates/tests/**/*.cjs",
];

export default defineConfig([
  globalIgnores([
    ".cache/**",
    ".codegraph/**",
    "dist/**",
    "node_modules/**",
    "reports/**",
    "target/**",
    "tests/environments/android/probe/build/**",
  ]),
  {
    extends: ["js/all"],
    files: javascriptFiles,
    languageOptions: {
      ecmaVersion: "latest",
      globals: globals.node,
      sourceType: "module",
    },
    linterOptions: {
      noInlineConfig: true,
      reportUnusedDisableDirectives: "error",
      reportUnusedInlineConfigs: "error",
    },
    name: "omarchy-quickshare/all-core-javascript-rules",
    plugins: { "@stylistic": stylistic, js, regexp },
    rules: {
      ...regexpRules,
      "@stylistic/max-len": [
        "error",
        { code: MAXIMUM_LINE_LENGTH, comments: MAXIMUM_LINE_LENGTH },
      ],
      "func-style": ["error", "declaration", { allowArrowFunctions: true }],
      "max-lines": [
        "error",
        {
          max: APPLICATION_FILE_LIMIT,
          skipBlankLines: false,
          skipComments: false,
        },
      ],
      "max-lines-per-function": [
        "error",
        {
          IIFEs: true,
          max: FUNCTION_LINE_LIMIT,
          skipBlankLines: true,
          skipComments: true,
        },
      ],
      "max-statements": ["error", MAXIMUM_STATEMENTS],
      "no-magic-numbers": [
        "error",
        {
          detectObjects: true,
          enforceConst: true,
          ignore: [-1, 0, 1, 2],
          ignoreArrayIndexes: true,
          ignoreDefaultValues: true,
        },
      ],
      "no-restricted-syntax": ["error", ...regexNamingRestrictions],
      "one-var": ["error", "never"],
      "sort-imports": [
        "error",
        {
          allowSeparatedGroups: true,
          ignoreCase: false,
          ignoreDeclarationSort: true,
          ignoreMemberSort: false,
          memberSyntaxSortOrder: ["none", "all", "multiple", "single"],
        },
      ],
      "sort-keys": [
        "error",
        "asc",
        {
          allowLineSeparatedGroups: true,
          caseSensitive: false,
          minKeys: LARGE_OBJECT_MINIMUM_KEYS,
          natural: true,
        },
      ],
    },
  },
  {
    files: testFiles,
    name: "omarchy-quickshare/test-javascript-limits",
    rules: {
      "max-lines": [
        "error",
        {
          max: TEST_FILE_LIMIT,
          skipBlankLines: true,
          skipComments: true,
        },
      ],
      "max-statements": ["error", TEST_MAXIMUM_STATEMENTS],
    },
  },
]);
