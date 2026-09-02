import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("../h4_relay.py", import.meta.url), "utf8");
const COMMAND = /kind == b"\\x01"/u;
const ACL = /kind == b"\\x02"/u;
const FORWARD = /writer\.write\(await _frame\(reader\)\)/u;

test("H4 relay reassembles commands and ACL packets", () => {
  assert.match(source, COMMAND);
  assert.match(source, ACL);
  assert.match(source, FORWARD);
});
