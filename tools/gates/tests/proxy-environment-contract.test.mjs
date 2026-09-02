import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  environmentFingerprint,
  trackUpstreamSocket,
  validateEnvironment,
} from "../../../tests/environments/proxies/environment.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const DIRECTORY = join(ROOT, "tests", "environments", "proxies");
const BASE_IMAGE_PATTERN = /^debian@sha256:/u;
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const SHA256_ERROR_PATTERN = /SHA-256 digest/u;
const ARTIFACT_WRITER_PATTERN = /recordFailureArtifact/u;
const TOXIPROXY_GATE_PATTERN = /toxiproxy/u;
const PROXY_PROOF_STAGE_PATTERN = /proxy-proof/u;

function inputs() {
  return {
    manifest: readFileSync(join(DIRECTORY, "environment.json"), "utf8"),
    dockerfile: readFileSync(join(DIRECTORY, "Dockerfile.toolchain"), "utf8"),
  };
}

test("proxy environment pins its image, snapshot, and source", () => {
  const { manifest, dockerfile } = inputs();
  const parsed = validateEnvironment(manifest, dockerfile);
  assert.match(parsed.base, BASE_IMAGE_PATTERN);
  assert.equal(parsed.go.version, "1.27.1");
  assert.match(parsed.go.sha256, SHA256_PATTERN);
  assert.match(parsed.source.revision, REVISION_PATTERN);
  assert.match(environmentFingerprint(manifest, dockerfile), SHA256_PATTERN);
});

test("proxy environment rejects a mutable base image", () => {
  const { manifest, dockerfile } = inputs();
  const changed = JSON.stringify({
    ...JSON.parse(manifest),
    base: "debian:trixie",
  });
  assert.throws(
    () => validateEnvironment(changed, dockerfile),
    SHA256_ERROR_PATTERN,
  );
});

test("proxy failures retain only typed artifact metadata", () => {
  const runner = readFileSync(join(DIRECTORY, "environment.mjs"), "utf8");
  assert.match(runner, ARTIFACT_WRITER_PATTERN);
  assert.match(runner, TOXIPROXY_GATE_PATTERN);
  assert.match(runner, PROXY_PROOF_STAGE_PATTERN);
  assert.equal(runner.includes("toxiproxy.log"), false);
});

test("proxy echo sockets classify expected connection resets", () => {
  const failures = [];
  const received = [];
  const socket = new EventEmitter();
  socket.end = () => null;
  trackUpstreamSocket(socket, received, failures);
  socket.emit(
    "error",
    Object.assign(new Error("reset"), {
      code: "ECONNRESET",
    }),
  );
  assert.deepEqual(failures, []);

  const broken = new EventEmitter();
  broken.end = () => null;
  trackUpstreamSocket(broken, received, failures);
  broken.emit(
    "error",
    Object.assign(new Error("broken"), {
      code: "EPIPE",
    }),
  );
  assert.deepEqual(failures, ["EPIPE"]);
});
