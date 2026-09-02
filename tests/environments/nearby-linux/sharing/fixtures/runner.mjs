import { createHash, randomUUID } from "node:crypto";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../../../tools/gates/lib/process.mjs";
import { parseSources } from "../../../../../tools/gates/sources.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../../../..");
const FIXTURES = join(ROOT, "tests", "fixtures", "sharing", "google-v1");
const ENVIRONMENT = join(DIRECTORY, "..", "..", "environment.mjs");
const VALIDATION_PATTERN =
  /Validated Nearby Linux environment (?<fingerprint>[a-f0-9]{64})\./u;
const FIXTURE_FILES = [
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
  "trace.json",
];
const MANIFEST = "manifest.json";

function sha256(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function fixtureFiles(directory) {
  for (const path of FIXTURE_FILES) {
    if (!existsSync(join(directory, path))) {
      throw new Error(`generated Sharing fixture is missing: ${path}`);
    }
  }
  return FIXTURE_FILES;
}

export function fixtureManifest(directory, provenance) {
  return {
    files: fixtureFiles(directory).map((path) => ({
      bytes: readFileSync(join(directory, path)).length,
      path,
      sha256: sha256(join(directory, path)),
    })),
    generator: provenance.generator,
    schema: 1,
    source: provenance.source,
  };
}

export function writeFixtureManifest(directory, manifest) {
  writeFileSync(
    join(directory, MANIFEST),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );
}

function compareFile(expected, actual, path) {
  const expectedBytes = readFileSync(join(expected, path));
  const actualBytes = readFileSync(join(actual, path));
  if (!expectedBytes.equals(actualBytes)) {
    throw new Error(`Sharing fixture ${path} differs`);
  }
}

function directoryFiles(directory, prefix = "") {
  const files = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const relative = join(prefix, entry.name);
    if (entry.isDirectory()) {
      files.push(...directoryFiles(join(directory, entry.name), relative));
    } else if (entry.isFile()) {
      files.push(relative);
    } else {
      throw new Error(`unsupported Sharing fixture entry: ${relative}`);
    }
  }
  return files.sort();
}

export function compareFixtures(expected, actual) {
  const expectedFiles = directoryFiles(expected);
  const actualFiles = directoryFiles(actual);
  if (expectedFiles.join("\0") !== actualFiles.join("\0")) {
    throw new Error("Sharing fixture directory contents differ");
  }
  for (const path of [...FIXTURE_FILES, MANIFEST]) {
    compareFile(expected, actual, path);
  }
}

function image() {
  return JSON.parse(
    readFileSync(join(DIRECTORY, "..", "..", "environment.json"), "utf8"),
  ).image;
}

export function assertCurrentFingerprint(actual, validation) {
  const expected = validation.match(VALIDATION_PATTERN)?.groups.fingerprint;
  if (!expected || actual !== expected) {
    throw new Error("Nearby Linux image is stale; run nearby-linux-provision");
  }
  return actual;
}

function assertImage() {
  const actual = output(process.env.DOCKER ?? "docker", [
    "image",
    "inspect",
    "--format",
    '{{index .Config.Labels "org.omarchy-quickshare.fingerprint"}}',
    image(),
  ]);
  const validation = output(process.execPath, [ENVIRONMENT, "validate"]);
  return assertCurrentFingerprint(actual, validation);
}

function sourceProvenance() {
  const records = parseSources(
    readFileSync(join(ROOT, "upstream", "sources.toml"), "utf8"),
  );
  const source = records.find((record) => record.id === "nearby-linux");
  if (!source) {
    throw new Error("nearby-linux source provenance is missing");
  }
  return source;
}

function generatorProvenance(environmentFingerprint) {
  return {
    environmentFingerprint,
    target: "//tools/quickshare_fixture_generator:sharing_fixture_generator",
  };
}

export function generatorArguments(directory) {
  return [
    "run",
    "--rm",
    "--cap-drop=ALL",
    "--security-opt=no-new-privileges",
    "--pids-limit=32",
    "--memory=128m",
    "--network=none",
    "--read-only",
    "--user",
    `${process.getuid()}:${process.getgid()}`,
    "--volume",
    `${directory}:/fixtures`,
    "--tmpfs",
    "/tmp:rw,noexec,nosuid,size=16m",
    "--entrypoint",
    "/usr/local/bin/sharing_fixture_generator",
    image(),
    "/fixtures",
  ];
}

function generate(directory) {
  const fingerprint = assertImage();
  run(process.env.DOCKER ?? "docker", generatorArguments(directory));
  return fingerprint;
}

function generatedDirectory(parent = tmpdir()) {
  return mkdtempSync(join(parent, ".quickshare-sharing-fixtures-"));
}

function addManifest(directory, environmentFingerprint) {
  writeFixtureManifest(
    directory,
    fixtureManifest(directory, {
      generator: generatorProvenance(environmentFingerprint),
      source: sourceProvenance(),
    }),
  );
}

function installFixtures(temporary) {
  const backup = join(dirname(FIXTURES), `.google-v1-backup-${randomUUID()}`);
  const hadFixtures = existsSync(FIXTURES);
  if (hadFixtures) {
    renameSync(FIXTURES, backup);
  }
  try {
    renameSync(temporary, FIXTURES);
  } catch (error) {
    if (hadFixtures) {
      renameSync(backup, FIXTURES);
    }
    throw error;
  }
  rmSync(backup, { force: true, recursive: true });
}

function update() {
  const temporary = generatedDirectory(dirname(FIXTURES));
  try {
    const fingerprint = generate(temporary);
    addManifest(temporary, fingerprint);
    installFixtures(temporary);
  } finally {
    rmSync(temporary, { force: true, recursive: true });
  }
}

function compare() {
  const temporary = generatedDirectory();
  try {
    const fingerprint = generate(temporary);
    addManifest(temporary, fingerprint);
    compareFixtures(FIXTURES, temporary);
  } finally {
    rmSync(temporary, { force: true, recursive: true });
  }
}

function main() {
  const [, , action] = process.argv;
  if (action === "compare") {
    compare();
  } else if (action === "update") {
    update();
  } else {
    throw new Error("expected Sharing fixture action: compare or update");
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
