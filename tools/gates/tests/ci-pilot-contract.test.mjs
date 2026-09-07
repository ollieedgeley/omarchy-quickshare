import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join } from "node:path";
import test from "node:test";

import { runPilot } from "../ci/pilot.mjs";

const PRIVATE_FILE_MODE = 0o600;
const FILE_TYPE_BOUNDARY = 0o1000;
const EXECUTABLE_MODE = 0o700;
const CHILD_FAILURE = 23;
const SECRET = "private-protocol-payload-never-publish";
const SHA_LENGTH = 40;

function git(root, args) {
  const result = spawnSync("git", ["-C", root, ...args], {
    encoding: "utf8",
    env: { PATH: process.env.PATH },
  });
  assert.equal(result.status, 0, result.stderr);
  return result.stdout.trim();
}

function executable(root, name, body) {
  writeFileSync(join(root, "bin", name), `#!${process.execPath}\n${body}\n`, {
    mode: EXECUTABLE_MODE,
  });
}

function installCapabilities(root) {
  executable(
    root,
    "node",
    "process.stdout.write(process.env.PILOT_NODE_VERSION ?? 'v26.7.0\\n');",
  );
  executable(
    root,
    "docker",
    `
if (process.env.PILOT_CAPTURE_REPORT) {
  const { copyFileSync } = require('node:fs');
  copyFileSync('.cache/ci-pilot/docker/cold/report.json', 'during-step.json');
}
process.stderr.write('${SECRET}\\n');
if (process.env.PILOT_DOCKER_DOWN === '1') process.exitCode = 1;
else process.stdout.write('Docker version 28.0.1\\n');`,
  );
  executable(root, "modinfo", "process.exitCode = 1;");
  executable(root, "python3", "process.exitCode = 1;");
}

function fixture(context) {
  const root = mkdtempSync(join(tmpdir(), "quickshare-ci-pilot-"));
  context.after(() => rmSync(root, { force: true, recursive: true }));
  mkdirSync(join(root, "bin"));
  installCapabilities(root);
  writeFileSync(
    join(root, "Makefile"),
    [
      ".PHONY: sources-fetch nearby-linux-provision rust-lan-provision",
      ".PHONY: test-rust-lan-outbound test-rust-lan-inbound",
      "sources-fetch nearby-linux-provision rust-lan-provision:",
      "\t@printf '%s\\n' '$@' >> prepared",
      "test-rust-lan-outbound:",
      `\t@printf '%s\\n' '${SECRET}'`,
      "\t@printf outbound >> observed",
      `\t@if test "$$PILOT_FAIL_TEST" = 1; then exit ${CHILD_FAILURE}; fi`,
      "test-rust-lan-inbound:",
      "\t@printf inbound >> observed",
      "",
    ].join("\n"),
  );
  git(root, ["init", "--quiet"]);
  git(root, ["add", "Makefile"]);
  git(root, [
    "-c",
    "user.name=Pilot contract",
    "-c",
    "user.email=pilot@invalid",
    "-c",
    "core.hooksPath=/dev/null",
    "commit",
    "--quiet",
    "-m",
    "fixture",
  ]);
  const env = { PATH: `${join(root, "bin")}${delimiter}${process.env.PATH}` };
  return { env, root, sha: git(root, ["rev-parse", "HEAD"]) };
}

function persisted(root, profile, attempt = "cold") {
  return JSON.parse(
    readFileSync(
      join(root, ".cache", "ci-pilot", profile, attempt, "report.json"),
      "utf8",
    ),
  );
}

test("invalid profiles and attempts cannot create files", (context) => {
  const options = fixture(context);
  assert.throws(() => runPilot("../escape", "cold", options));
  assert.throws(() => runPilot("docker", "../escape", options));
  assert.equal(existsSync(join(options.root, ".cache")), false);
});

test("missing Docker fails before preparation", (context) => {
  const options = fixture(context);
  options.env.PILOT_DOCKER_DOWN = "1";
  const report = runPilot("docker", "cold", options);
  assert.equal(report.status, "failed");
  assert.equal(report.failure.id, "docker-client");
  assert.equal(report.capabilities["docker-client"].available, false);
  assert.equal(existsSync(join(options.root, "prepared")), false);
  assert.deepEqual(persisted(options.root, "docker"), report);
  assert.equal(JSON.stringify(report).includes(SECRET), false);
  options.env.PILOT_NODE_VERSION = "v26.6.0";
  const wrongPin = runPilot("docker", "warm", options);
  assert.equal(wrongPin.failure.id, "host-node");
  assert.deepEqual(wrongPin.capabilities["host-node"], {
    available: false,
    expected: "26.7.0",
    satisfied: false,
    version: "26.6.0",
  });
});

test("kernel and KVM failures prevent provisioning", (context) => {
  const options = fixture(context);
  const network = runPilot("network", "cold", options);
  const kvm = runPilot("kvm", "cold", options);
  assert.equal(network.status, "failed");
  assert.equal(network.failure.id, "hwsim-module");
  assert.equal(kvm.status, "failed");
  assert.equal(kvm.failure.id, "kvm-access");
  assert.equal(existsSync(join(options.root, "prepared")), false);
});

test("child failure stops execution and keeps logs private", (context) => {
  const options = fixture(context);
  options.env.PILOT_FAIL_TEST = "1";
  const report = runPilot("docker", "cold", options);
  assert.equal(report.status, "failed");
  assert.equal(report.failure.id, "test-rust-lan-outbound");
  assert.notEqual(report.failure.exitCode, 0);
  assert.equal(
    readFileSync(join(options.root, "observed"), "utf8"),
    "outbound",
  );
  const log = join(
    options.root,
    ".cache",
    "ci-pilot-logs",
    "docker",
    "cold",
    "test-rust-lan-outbound.log",
  );
  assert.equal(readFileSync(log, "utf8").includes(SECRET), true);
  assert.equal(statSync(log).mode % FILE_TYPE_BOUNDARY, PRIVATE_FILE_MODE);
  assert.equal(
    JSON.stringify(persisted(options.root, "docker")).includes(SECRET),
    false,
  );
});

test("source revision mismatch fails before preparation", (context) => {
  const options = fixture(context);
  options.env.CI_PILOT_SOURCE_SHA = "f".repeat(SHA_LENGTH);
  options.env.RUNNER_ENVIRONMENT = SECRET;
  options.env.ImageVersion = SECRET;
  const report = runPilot("docker", "cold", options);
  assert.equal(report.status, "failed");
  assert.equal(report.failure.id, "source-identity");
  assert.equal(existsSync(join(options.root, "prepared")), false);
  assert.equal(report.provider.runner, "unknown");
  assert.equal(report.provider.imageVersion, null);
  assert.equal(JSON.stringify(report).includes(SECRET), false);
});

test("cold and warm reports retain independent executions", (context) => {
  const options = fixture(context);
  options.env.CI_PILOT_SOURCE_SHA = options.sha;
  options.env.CI_PILOT_HEAD_SHA = options.sha;
  options.env.CI_PILOT_BASE_SHA = options.sha;
  options.env.PRIVATE_VALUE = SECRET;
  options.env.PILOT_CAPTURE_REPORT = "1";
  const cold = runPilot("docker", "cold", options);
  assert.equal(cold.provider.runner, "local");
  assert.equal(cold.provider.imageVersion, null);
  const during = JSON.parse(
    readFileSync(join(options.root, "during-step.json"), "utf8"),
  );
  assert.equal(during.status, "running");
  assert.equal(during.steps.at(-1).status, "running");
  assert.equal(during.capabilities["docker-client"].available, true);
  const coldBytes = JSON.stringify(persisted(options.root, "docker"));
  options.env.RUNNER_ENVIRONMENT = "github-hosted";
  options.env.ImageVersion = "20260901.12.1";
  const warm = runPilot("docker", "warm", options);
  assert.equal(cold.status, "passed");
  assert.equal(warm.status, "passed");
  assert.equal(warm.provider.runner, "github-hosted");
  assert.equal(warm.provider.imageVersion, "20260901.12.1");
  assert.deepEqual(warm.source, {
    actual: options.sha,
    base: options.sha,
    dirty: true,
    head: options.sha,
  });
  assert.equal(
    readFileSync(join(options.root, "observed"), "utf8"),
    "outboundinboundoutboundinbound",
  );
  assert.equal(coldBytes, JSON.stringify(persisted(options.root, "docker")));
  assert.equal(persisted(options.root, "docker", "warm").attempt, "warm");
  assert.throws(() => runPilot("docker", "cold", options));
  assert.equal(coldBytes, JSON.stringify(persisted(options.root, "docker")));
  assert.equal(JSON.stringify(warm).includes(SECRET), false);
  for (const step of warm.steps) {
    assert.ok(Number.isInteger(step.durationMs) && step.durationMs >= 0);
  }
});
