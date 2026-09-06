import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const CONTINUATION = /\\\r?\n\s*/gu;
const LINE_BREAK = /\r?\n/u;
const REGISTRY_CACHE =
  /--mount=type=cache,target=\/usr\/local\/cargo\/registry,sharing=locked/u;
const TARGET_CACHE =
  /--mount=type=cache,target=\/workspace\/target,sharing=locked/u;
const CARGO_BUILD =
  /cargo build --locked --release --package omarchy-quickshare/u;
const EXPORT_BINARY = new RegExp(
  "&& cp target/release/omarchy-quickshare " +
    "/usr/local/bin/omarchy-quickshare",
  "u",
);
const COPY_BINARY =
  /COPY --from=builder (?<p>\/usr\/local\/bin\/omarchy-quickshare)\s+\k<p>/u;
const TOOLING_SELECTION =
  /tests\/environments\/diverse-lan\/rust\/build-cache\.test\.mjs/u;

function source(name) {
  return readFileSync(new URL(name, import.meta.url), "utf8");
}

test("Rust peer caches Cargo builds and exports the cached binary", () => {
  const dockerfile = source("Dockerfile.rust-peer").replace(CONTINUATION, " ");
  const build = dockerfile
    .split(LINE_BREAK)
    .find((line) => CARGO_BUILD.test(line));
  assert.ok(build, "locked release build remains present");
  assert.match(build, REGISTRY_CACHE);
  assert.match(build, TARGET_CACHE);
  assert.match(build, EXPORT_BINARY);
  assert.match(dockerfile, COPY_BINARY);
});

test("Rust peer build context excludes non-build repository trees", () => {
  const ignored = new Set(
    source("Dockerfile.rust-peer.dockerignore").trim().split(LINE_BREAK),
  );
  for (const directory of [
    "tests",
    "tools",
    "fuzz",
    ".cache",
    "docs",
    "packaging",
    "reports",
    "node_modules",
    ".git",
    "upstream",
    "target",
    "dist",
  ]) {
    assert.ok(ignored.has(directory), `${directory} must stay outside COPY`);
  }
});

test("tooling tests select the rust peer cache contracts", () => {
  const packageManifest = JSON.parse(
    readFileSync(new URL("../../../../package.json", import.meta.url), "utf8"),
  );
  assert.match(packageManifest.scripts["test:tooling"], TOOLING_SELECTION);
});
