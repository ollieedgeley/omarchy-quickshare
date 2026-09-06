import assert from "node:assert/strict";
import { EventEmitter, once } from "node:events";
import { readFileSync } from "node:fs";
import { connect, createServer } from "node:net";
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

test("proxy upstream echoes the whole TCP connection", async () => {
  const fragmentBytes = 7;
  const timeoutMs = 1000;
  const payload = Buffer.from("fragmented-proxy-payload");
  const received = [];
  const failures = [];
  const upstream = createServer((socket) => {
    trackUpstreamSocket(socket, received, failures);
  });
  upstream.listen(0, "127.0.0.1");
  await once(upstream, "listening");
  try {
    const echoed = await new Promise((accept, reject) => {
      const chunks = [];
      let echoedBytes = 0;
      const socket = connect(upstream.address().port, "127.0.0.1", () => {
        socket.write(payload.subarray(0, fragmentBytes));
      });
      socket.setTimeout(timeoutMs, () => {
        socket.destroy(new Error("fragmented echo timed out"));
      });
      socket.on("error", reject);
      socket.once("data", () => socket.write(payload.subarray(fragmentBytes)));
      socket.on("data", (chunk) => {
        chunks.push(chunk);
        echoedBytes += chunk.length;
        if (echoedBytes >= payload.length) {
          socket.end();
        }
      });
      socket.on("close", () => accept(Buffer.concat(chunks)));
    });
    assert.deepEqual(echoed, payload);
    await new Promise((accept, reject) => {
      const socket = connect(upstream.address().port, "127.0.0.1", () => {
        socket.end();
      });
      socket.setTimeout(timeoutMs, () => {
        socket.destroy(new Error("empty connection timed out"));
      });
      socket.on("error", reject);
      socket.on("close", accept);
    });
    assert.deepEqual(received, [payload.length, 0]);
    assert.deepEqual(failures, []);
  } finally {
    upstream.close();
    await once(upstream, "close");
  }
});
