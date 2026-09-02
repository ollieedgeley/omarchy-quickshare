import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

import {
  assertCurrentFingerprint,
  compareFixtures,
  fixtureManifest,
  generatorArguments,
  writeFixtureManifest,
} from "./runner.mjs";
import { contextFingerprint } from "../../context.mjs";

const SOURCE_REVISION = "6887b0983200c6c8c29e614ea2633d13bf18315d";
const DIRECTORY = new URL(".", import.meta.url);
const MANIFEST_TEST =
  "Sharing fixture manifest records exact bytes and pinned provenance";
const SHA256_PATTERN = /^[a-f0-9]{64}$/u;
const DIFFERENCE_PATTERN = /outgoing\/introductions\/file\.bin differs/u;
const WIRE_PROTO_PATTERN = /wire_format_cc_proto/u;
const OUTBOUND_FILE_PATTERN = /OutgoingShareSession/u;
const INBOUND_APK_PATTERN = /IntroductionFrame Apk/u;
const DIRECTORY_DIFFERENCE_PATTERN = /fixture directory contents differ/u;
const STALE_IMAGE_PATTERN = /Nearby Linux image is stale/u;
const FINGERPRINT_LENGTH = 64;
const TRACE_RECORDS = 13;
const TRACE_SCHEMA = 5;
const TRACE = new URL(
  "../../../../fixtures/sharing/google-v1/trace.json",
  import.meta.url,
);

function fixtureDirectory() {
  return mkdtempSync(join(tmpdir(), "sharing-fixtures-"));
}

function writeFixtures(directory, outbound = "outbound", inbound = "inbound") {
  const names = [
    "incoming/introductions/apk.bin",
    "incoming/introductions/file.bin",
    "incoming/introductions/text.bin",
    "incoming/introductions/url.bin",
    "incoming/responses/accept.bin",
    "incoming/responses/cancel.bin",
    "incoming/responses/not-enough-space.bin",
    "incoming/responses/reject.bin",
    "incoming/responses/timed-out.bin",
    "incoming/responses/unsupported.bin",
    "outgoing/introductions/file.bin",
    "outgoing/introductions/text.bin",
    "outgoing/introductions/url.bin",
  ];
  for (const name of names) {
    mkdirSync(dirname(join(directory, name)), { recursive: true });
    writeFileSync(join(directory, name), "fixture");
  }
  writeFileSync(join(directory, "outgoing/introductions/file.bin"), outbound);
  writeFileSync(join(directory, "incoming/responses/accept.bin"), inbound);
  writeFileSync(join(directory, "trace.json"), '{"schema":1}\n');
}

test(MANIFEST_TEST, () => {
  const directory = fixtureDirectory();
  try {
    writeFixtures(directory);
    const manifest = fixtureManifest(directory, {
      generator: { target: "//tools/quickshare_fixture_generator" },
      source: { id: "nearby-linux", revision: SOURCE_REVISION },
    });
    writeFixtureManifest(directory, manifest);
    const saved = JSON.parse(readFileSync(join(directory, "manifest.json")));
    assert.equal(saved.source.revision, SOURCE_REVISION);
    assert.equal(saved.files[0].path, "incoming/introductions/apk.bin");
    assert.equal(saved.files.at(-1).path, "trace.json");
    assert.match(saved.files[0].sha256, SHA256_PATTERN);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

test("Sharing fixture trace records every decoded frame once", () => {
  const trace = JSON.parse(readFileSync(TRACE, "utf8"));
  assert.equal(trace.schema, TRACE_SCHEMA);
  assert.equal(trace.records.length, TRACE_RECORDS);
  assert.deepEqual(trace.records.map((record) => record.path).sort(), [
    "incoming/introductions/apk.bin",
    "incoming/introductions/file.bin",
    "incoming/introductions/text.bin",
    "incoming/introductions/url.bin",
    "incoming/responses/accept.bin",
    "incoming/responses/cancel.bin",
    "incoming/responses/not-enough-space.bin",
    "incoming/responses/reject.bin",
    "incoming/responses/timed-out.bin",
    "incoming/responses/unsupported.bin",
    "outgoing/introductions/file.bin",
    "outgoing/introductions/text.bin",
    "outgoing/introductions/url.bin",
  ]);
  for (const record of trace.records) {
    assert.ok(record.kind);
    assert.ok(record.outcome);
  }
});

test("Sharing fixture comparison rejects an unrecorded byte change", () => {
  const expected = fixtureDirectory();
  const actual = fixtureDirectory();
  try {
    writeFixtures(expected);
    writeFixtures(actual, "changed");
    assert.throws(() => compareFixtures(expected, actual), DIFFERENCE_PATTERN);
  } finally {
    rmSync(expected, { force: true, recursive: true });
    rmSync(actual, { force: true, recursive: true });
  }
});

test("Sharing fixture comparison rejects an unexpected file", () => {
  const expected = fixtureDirectory();
  const actual = fixtureDirectory();
  try {
    writeFixtures(expected);
    writeFixtures(actual);
    writeFileSync(join(actual, "obsolete.bin"), "obsolete");
    assert.throws(
      () => compareFixtures(expected, actual),
      DIRECTORY_DIFFERENCE_PATTERN,
    );
  } finally {
    rmSync(expected, { force: true, recursive: true });
    rmSync(actual, { force: true, recursive: true });
  }
});

test("Sharing fixture generation rejects a stale peer image", () => {
  const current = "a".repeat(FINGERPRINT_LENGTH);
  const stale = "b".repeat(FINGERPRINT_LENGTH);
  const validation = `Validated Nearby Linux environment ${current}.`;
  assert.equal(assertCurrentFingerprint(current, validation), current);
  assert.throws(
    () => assertCurrentFingerprint(stale, validation),
    STALE_IMAGE_PATTERN,
  );
});

test("Sharing fixture generation drops privileges and external access", () => {
  const argumentsList = generatorArguments("/tmp/fixture-case");
  for (const value of [
    "--cap-drop=ALL",
    "--security-opt=no-new-privileges",
    "--network=none",
    "--read-only",
    "--pids-limit=32",
    "--memory=128m",
  ]) {
    assert.ok(argumentsList.includes(value));
  }
  assert.ok(argumentsList.includes("/tmp/fixture-case:/fixtures"));
});

test("Sharing fixture generator is a sealed Nearby Linux image input", () => {
  const generator = new URL(
    "../../../../../tools/oracle/sharing-fixtures/",
    DIRECTORY,
  );
  const build = readFileSync(new URL("BUILD.bazel", generator), "utf8");
  const source = readFileSync(
    new URL("sharing_fixture_generator.cc", generator),
    "utf8",
  );
  assert.ok(existsSync(generator));
  assert.match(build, WIRE_PROTO_PATTERN);
  assert.match(source, OUTBOUND_FILE_PATTERN);
  assert.match(source, INBOUND_APK_PATTERN);
});

test("Sharing fixture generator changes the sealed context fingerprint", () => {
  const input = {
    assets: "assets",
    compose: "compose",
    connectionsPeer: "connections-peer",
    contextSource: "context",
    dockerfile: "dockerfile",
    fixtureGenerator: "generator-a",
    manifestSource: "manifest",
    overlays: "overlays",
    patch: "patch",
    sources: "sources",
  };
  assert.notEqual(
    contextFingerprint(input),
    contextFingerprint({ ...input, fixtureGenerator: "generator-b" }),
  );
});
