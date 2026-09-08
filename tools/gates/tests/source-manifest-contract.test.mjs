import assert from "node:assert/strict";
import { test } from "vitest";

import { parseSources } from "../sources.mjs";

const MUTABLE_URL_PATTERN = /URL is not pinned/u;
const DUPLICATE_SOURCE_PATTERN = /duplicate source id/u;

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
