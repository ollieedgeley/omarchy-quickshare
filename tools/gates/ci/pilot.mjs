import {
  copyFileSync,
  mkdirSync,
  renameSync,
  statfsSync,
  writeFileSync,
} from "node:fs";
import { arch, cpus, release } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../lib/process.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const PROFILES = ["core", "docker", "kvm", "network"];
const ATTEMPTS = ["cold", "warm"];
const SHA = /^[0-9a-f]{40}$/u;
const VERSION = /(?<!\d)\d{1,3}\.\d{1,3}(?:\.\d{1,3})?\b/u;
const IMAGE = "quickshare-ci-pilot:arch-20260825";
const PRIVATE_MODE = 0o700;
const MAX_CLI_ARGUMENTS = 4;
const MAX_REPORT_BYTES = 65_536;
const RUNNER_ENVIRONMENTS = ["github-hosted", "self-hosted"];
const IMAGE_VERSION = /^\d{8}\.\d{1,3}\.\d{1,3}$/u;

function disk(root) {
  const stats = statfsSync(root);
  return {
    availableBytes: stats.bavail * stats.bsize,
    totalBytes: stats.blocks * stats.bsize,
  };
}

function optionalSha(value) {
  if (!value) {
    return null;
  }
  if (!SHA.test(value)) {
    throw new Error("invalid-source-identity");
  }
  return value;
}

function sourceIdentity(root, env) {
  const result = run("git", ["rev-parse", "HEAD"], {
    capture: true,
    cwd: root,
    env,
    quiet: true,
  });
  const actual = result.stdout.trim();
  if (!SHA.test(actual)) {
    throw new Error("invalid-source-identity");
  }
  const expected = optionalSha(env.CI_PILOT_SOURCE_SHA);
  if (expected && expected !== actual) {
    throw new Error("source-identity-mismatch");
  }
  const status = run(
    "git",
    ["status", "--porcelain", "--untracked-files=normal"],
    {
      capture: true,
      cwd: root,
      env,
      quiet: true,
    },
  );
  return {
    actual,
    base: optionalSha(env.CI_PILOT_BASE_SHA),
    dirty: status.stdout.length !== 0,
    head: optionalSha(env.CI_PILOT_HEAD_SHA),
  };
}

function makeStep(target, kind = "prepare") {
  return { args: [target], command: "make", id: target, kind };
}

function containerStep(root, work, step) {
  return {
    ...step,
    args: [
      "run",
      "--rm",
      "--read-only",
      "--user",
      `${process.getuid()}:${process.getgid()}`,
      "--tmpfs",
      "/tmp:rw,exec,mode=1777",
      "--volume",
      `${root}:/workspace:ro`,
      "--volume",
      `${join(work, "npm", "node_modules")}:/workspace/node_modules:ro`,
      "--volume",
      `${join(work, "cargo")}:/cargo:rw`,
      "--volume",
      `${join(work, "target")}:/target:rw`,
      "--workdir",
      "/workspace",
      "--env",
      "CARGO_HOME=/cargo",
      "--env",
      "CARGO_TARGET_DIR=/target",
      "--env",
      "HOME=/tmp/home",
      "--env",
      "XDG_RUNTIME_DIR=/tmp/runtime",
      "--env",
      "QT_QPA_PLATFORM=offscreen",
      "--env",
      "QT_QUICK_BACKEND=software",
      IMAGE,
      step.command,
      ...step.args,
    ],
    command: "docker",
  };
}

function coreCommands() {
  return [
    {
      args: ["--version"],
      command: "rustc",
      expected: "1.98.0",
      id: "core-rust-version",
      kind: "capability",
    },
    {
      args: ["--version"],
      command: "node",
      expected: "26.7.0",
      id: "core-node-version",
      kind: "capability",
    },
    {
      args: ["--version"],
      command: "quickshell",
      expected: "0.3.1",
      id: "core-quickshell-version",
      kind: "capability",
    },
    {
      args: [
        "test",
        "-p",
        "quickshare-contract-tests",
        "--test",
        "suite",
        "--locked",
        "--no-run",
      ],
      command: "cargo",
      id: "contract-build",
      kind: "prepare",
    },
    ...["lint-structure-app", "test-contracts", "test-plugin-release"].map(
      (target) => makeStep(target, "test"),
    ),
  ];
}

function coreSteps(root, work) {
  return [
    {
      args: ["ci", "--no-audit", "--no-fund"],
      command: "npm",
      cwd: join(work, "npm"),
      id: "npm-ci",
      kind: "prepare",
    },
    {
      args: ["build", "--tag", IMAGE, join(root, "tools/gates/ci")],
      command: "docker",
      id: "core-image",
      kind: "prepare",
    },
    ...coreCommands().map((step) => containerStep(root, work, step)),
  ];
}

function profileSteps(profile, root, work) {
  if (profile === "core") {
    return coreSteps(root, work);
  }
  const steps = [makeStep("sources-fetch")];
  if (profile === "docker") {
    steps.push(
      makeStep("nearby-linux-provision"),
      makeStep("rust-lan-provision"),
      makeStep("test-rust-lan-outbound", "test"),
      makeStep("test-rust-lan-inbound", "test"),
    );
  } else if (profile === "kvm") {
    steps.push(
      makeStep("bluetooth-radio-provision"),
      makeStep("test-bluetooth-controller", "test"),
    );
  } else {
    steps.push(
      makeStep("network-provision"),
      makeStep("test-network-wmediumd", "test"),
    );
  }
  return steps;
}

function capabilitySteps(profile) {
  const steps = [
    {
      args: ["--version"],
      command: "node",
      expected: "26.7.0",
      id: "host-node",
    },
    { args: ["--version"], command: "git", id: "host-git" },
    { args: ["--version"], command: "make", id: "host-make" },
    { args: ["--version"], command: "docker", id: "docker-client" },
    {
      args: ["version", "--format", "{{.Server.Version}}"],
      command: "docker",
      id: "docker-server",
    },
  ];
  if (profile === "kvm") {
    steps.push({
      args: [
        "-c",
        "import os,fcntl; fd=os.open('/dev/kvm',os.O_RDWR); " +
          "version=fcntl.ioctl(fd,0xAE00,0); " +
          "os.close(fd); assert version == 12",
      ],
      command: "python3",
      id: "kvm-access",
    });
  }
  if (profile === "network") {
    steps.push(
      {
        args: ["-k", release(), "mac80211_hwsim"],
        command: "modinfo",
        id: "hwsim-module",
      },
      { args: ["-n", "true"], command: "sudo", id: "network-sudo" },
      { args: ["--version"], command: "iw", id: "network-iw" },
    );
  }
  return steps.map((step) => ({ ...step, kind: "capability" }));
}

function childOptions(step, context) {
  const options = {
    allowFailure: true,
    cwd: step.cwd ?? context.root,
    env: context.env,
    quiet: true,
  };
  if (step.kind === "capability") {
    options.capture = true;
  } else {
    options.logPath = join(context.logs, `${step.id}.log`);
  }
  return options;
}

function recordCapability(step, context, result) {
  const version =
    `${result.stdout ?? ""}${result.stderr ?? ""}`.match(VERSION)?.[0] ?? null;
  const satisfied =
    result.status === 0 && (!step.expected || version === step.expected);
  context.report.capabilities[step.id] = {
    available: satisfied,
    expected: step.expected ?? null,
    satisfied,
    version,
  };
  if (!satisfied) {
    throw new Error("capability-unsatisfied");
  }
}

function dispatch(step, context) {
  let deadline = "20m";
  if (step.kind === "capability") {
    deadline = "30s";
  }
  return run(
    "timeout",
    ["--kill-after=10s", deadline, step.command, ...step.args],
    childOptions(step, context),
  );
}

function writeReport(directory, report) {
  const json = `${JSON.stringify(report, null, 2)}\n`;
  if (Buffer.byteLength(json) > MAX_REPORT_BYTES) {
    throw new Error("pilot-report-too-large");
  }
  const temporary = join(directory, "report.json.temporary");
  writeFileSync(temporary, json);
  renameSync(temporary, join(directory, "report.json"));
  const summary = [
    "# Runner pilot",
    "",
    `Profile: ${report.profile}`,
    `Attempt: ${report.attempt}`,
    `Status: ${report.status}`,
    `Source: ${report.source?.actual ?? "unverified"}`,
    "",
    "| Step | Kind | Status | Duration ms |",
    "| --- | --- | --- | ---: |",
    ...report.steps.map(
      (step) =>
        `| ${step.id} | ${step.kind} | ${step.status} | ${step.durationMs} |`,
    ),
    "",
  ];
  writeFileSync(join(directory, "summary.md"), summary.join("\n"));
}

function execute(step, context) {
  const started = performance.now();
  const record = {
    durationMs: 0,
    exitCode: null,
    id: step.id,
    kind: step.kind,
    status: "running",
  };
  context.report.steps.push(record);
  context.report.status = "running";
  writeReport(context.directory, context.report);
  try {
    const result = dispatch(step, context);
    record.exitCode = result.status;
    if (step.kind === "capability") {
      recordCapability(step, context, result);
    }
    if (result.status !== 0) {
      throw new Error("child-failed");
    }
    record.status = "passed";
  } catch {
    record.status = "failed";
    context.report.status = "failed";
    if (step.kind === "capability") {
      context.report.capabilities[step.id] ??= {
        available: false,
        satisfied: false,
      };
    }
    context.report.failure = {
      exitCode: record.exitCode,
      id: step.id,
      kind: step.kind,
    };
    throw new Error("pilot-step-failed");
  } finally {
    record.durationMs = Math.round(performance.now() - started);
    writeReport(context.directory, context.report);
  }
}

function providerFacts(env) {
  let runner = "unknown";
  if (RUNNER_ENVIRONMENTS.includes(env.RUNNER_ENVIRONMENT)) {
    runner = env.RUNNER_ENVIRONMENT;
  } else if (!env.RUNNER_ENVIRONMENT && env.GITHUB_ACTIONS !== "true") {
    runner = "local";
  }
  let imageVersion = null;
  if (
    runner === "github-hosted" &&
    IMAGE_VERSION.test(env.ImageVersion ?? "")
  ) {
    imageVersion = env.ImageVersion;
  }
  return {
    candidate: "github-hosted-ubuntu-24.04",
    imageVersion,
    kvm: "experimental-not-provider-supported",
    runner,
  };
}

function startReport(profile, attempt, root) {
  return {
    attempt,
    capabilities: {},
    disk: { before: disk(root) },
    failure: null,
    host: {
      architecture: arch(),
      kernel: release(),
      logicalCpus: cpus().length,
    },
    profile,
    provider: null,
    schema: 1,
    source: null,
    startedAt: new Date().toISOString(),
    status: "started",
    steps: [],
  };
}

function prepareCore(root, work) {
  for (const child of ["npm", "cargo", "target"]) {
    mkdirSync(join(work, child), { mode: PRIVATE_MODE, recursive: true });
  }
  for (const name of ["package.json", "package-lock.json"]) {
    copyFileSync(join(root, name), join(work, "npm", name));
  }
}

export function runPilot(
  profile,
  attempt = "cold",
  { root = ROOT, env = process.env } = {},
) {
  if (!PROFILES.includes(profile) || !ATTEMPTS.includes(attempt)) {
    throw new Error("invalid pilot profile or attempt");
  }
  const directory = join(root, ".cache", "ci-pilot", profile, attempt);
  const logs = join(root, ".cache", "ci-pilot-logs", profile, attempt);
  const work = join(root, ".cache", "ci-pilot-work", profile);
  mkdirSync(dirname(directory), { recursive: true });
  mkdirSync(directory);
  mkdirSync(logs, { mode: PRIVATE_MODE, recursive: true });
  const report = startReport(profile, attempt, root);
  report.provider = providerFacts(env);
  const started = performance.now();
  const context = { directory, env, logs, report, root };
  writeReport(directory, report);
  try {
    report.source = sourceIdentity(root, env);
    for (const step of capabilitySteps(profile)) {
      execute(step, context);
    }
    if (profile === "core") {
      prepareCore(root, work);
    }
    for (const step of profileSteps(profile, root, work)) {
      execute(step, context);
    }
    report.status = "passed";
  } catch {
    report.status = "failed";
    let id = "source-identity";
    if (report.source) {
      id = "pilot-prepare";
    }
    report.failure ??= { exitCode: null, id, kind: "prepare" };
  } finally {
    report.disk.after = disk(root);
    report.durationMs = Math.round(performance.now() - started);
    report.finishedAt = new Date().toISOString();
    writeReport(directory, report);
  }
  return report;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length > MAX_CLI_ARGUMENTS) {
      throw new Error("invalid pilot arguments");
    }
    const report = runPilot(process.argv[2], process.argv[3]);
    process.stdout.write(
      `Pilot ${report.profile}/${report.attempt}: ${report.status}\n`,
    );
    if (report.status !== "passed") {
      process.exitCode = 1;
    }
  } catch {
    process.stderr.write(
      "Pilot rejected input or could not create its report.\n",
    );
    process.exitCode = 1;
  }
}
