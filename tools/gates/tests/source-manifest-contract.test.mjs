import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { parseSources } from "../sources.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const MINIMUM_SOURCE_COUNT = 18;
const MUTABLE_URL_PATTERN = /URL is not pinned/u;
const DUPLICATE_SOURCE_PATTERN = /duplicate source id/u;

test("source manifest pins every archive by revision and SHA-256", () => {
  const records = parseSources(
    readFileSync(join(ROOT, "upstream", "sources.toml"), "utf8"),
  );
  assert.ok(records.length >= MINIMUM_SOURCE_COUNT);
  assert.ok(records.some(({ id }) => id === "google-nearby"));
  assert.ok(records.some(({ id }) => id === "google-ukey2"));
  assert.ok(records.some(({ id }) => id === "nearby-linux"));
});

test("source manifest rejects mutable and duplicate inputs", () => {
  const record = `
schema = 1

[[source]]
id = "mutable"
url = "https://example.invalid/main.tar.gz"
revision = "0123456789abcdef0123456789abcdef01234567"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
license = "Apache-2.0"
purpose = "Contract fixture"
`;
  assert.throws(() => parseSources(record), MUTABLE_URL_PATTERN);
  const pinned = record.replace(
    "main.tar.gz",
    "0123456789abcdef0123456789abcdef01234567.tar.gz",
  );
  assert.throws(
    () => parseSources(`${pinned}\n${pinned.replace("schema = 1", "")}`),
    DUPLICATE_SOURCE_PATTERN,
  );
});
