import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { CONNECTIONS_PEER_FILES } from "./context.mjs";

const BUILD = new URL(
  "../../../tools/oracle/connections-peer/BUILD.bazel",
  import.meta.url,
);
const TARGET_INPUT_PATTERN =
  /^\s+"(?<name>connections_peer(?:_[a-z]+)?\.(?:cc|h))",$/gmu;

test("Nearby context copies every Connections peer build input", () => {
  const source = readFileSync(BUILD, "utf8");
  const targetInputs = [...source.matchAll(TARGET_INPUT_PATTERN)].map(
    ({ groups }) => groups.name,
  );
  const copiedInputs = CONNECTIONS_PEER_FILES.filter(
    (name) => name !== "BUILD.bazel",
  );

  assert.deepEqual(new Set(copiedInputs), new Set(targetInputs));
});
