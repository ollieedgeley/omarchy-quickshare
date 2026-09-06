import test from "node:test";
test("first fails", () => { throw new Error("expected"); });
