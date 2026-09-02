import { createHash } from "node:crypto";
import {
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
} from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "./lib/process.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const MANIFEST = join(ROOT, "upstream", "sources.toml");
const CACHE = resolve(
  process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env"),
  "sources",
);
const REQUIRED_KEYS = ["id", "url", "revision", "sha256", "license", "purpose"];

function stringValue(line, lineNumber) {
  const match = line.match(/^([a-z0-9_]+) = "([^"\n]*)"$/);
  if (!match) throw new Error(`invalid source manifest line ${lineNumber}`);
  return [match[1], match[2]];
}

export function parseSources(source) {
  const records = [];
  let current;
  let schema;
  for (const [index, raw] of source.split("\n").entries()) {
    const line = raw.trim();
    if (!line || line.startsWith("#")) continue;
    if (line === "[[source]]") {
      current = {};
      records.push(current);
      continue;
    }
    if (!current) {
      const match = line.match(/^schema = ([0-9]+)$/);
      if (!match || schema !== undefined) {
        throw new Error(`invalid source manifest line ${index + 1}`);
      }
      schema = Number(match[1]);
      continue;
    }
    const [key, value] = stringValue(line, index + 1);
    if (!REQUIRED_KEYS.includes(key) || Object.hasOwn(current, key)) {
      throw new Error(`unexpected source field ${key} on line ${index + 1}`);
    }
    current[key] = value;
  }
  if (schema !== 1) throw new Error(`unsupported source schema ${schema}`);
  validateSources(records);
  return records;
}

export function validateSources(records) {
  const ids = new Set();
  for (const record of records) {
    for (const key of REQUIRED_KEYS) {
      if (!record[key])
        throw new Error(`source ${record.id ?? "<unknown>"} lacks ${key}`);
    }
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(record.id)) {
      throw new Error(`invalid source id ${record.id}`);
    }
    if (ids.has(record.id)) throw new Error(`duplicate source id ${record.id}`);
    ids.add(record.id);
    if (
      !/^https:\/\//.test(record.url) ||
      !record.url.includes(record.revision)
    ) {
      throw new Error(`source ${record.id} URL is not pinned to its revision`);
    }
    if (!/^[0-9a-f]{40}$/.test(record.revision)) {
      throw new Error(`source ${record.id} has an invalid revision`);
    }
    if (!/^[0-9a-f]{64}$/.test(record.sha256)) {
      throw new Error(`source ${record.id} has an invalid SHA-256`);
    }
  }
  if (records.length === 0) throw new Error("source manifest is empty");
}

function sources() {
  return parseSources(readFileSync(MANIFEST, "utf8"));
}

function archivePath(record) {
  return join(CACHE, "archives", `${record.id}.tar.gz`);
}

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function assertArchive(record) {
  const path = archivePath(record);
  if (!existsSync(path)) {
    throw new Error(`missing ${record.id}; run \`make sources-fetch\``);
  }
  const actual = await sha256(path);
  if (actual !== record.sha256) {
    throw new Error(
      `${record.id} SHA-256 mismatch: expected ${record.sha256}, received ${actual}`,
    );
  }
}

async function fetchSource(record) {
  mkdirSync(join(CACHE, "archives"), { recursive: true });
  const destination = archivePath(record);
  if (!existsSync(destination)) {
    const temporary = mkdtempSync(
      join(CACHE, "archives", `.quickshare-${record.id}-`),
    );
    const download = join(temporary, basename(destination));
    try {
      run("curl", [
        "--fail",
        "--location",
        "--retry",
        "3",
        "--output",
        download,
        record.url,
      ]);
      const actual = await sha256(download);
      if (actual !== record.sha256) {
        throw new Error(
          `${record.id} SHA-256 mismatch: expected ${record.sha256}, received ${actual}`,
        );
      }
      renameSync(download, destination);
    } finally {
      rmSync(temporary, { recursive: true, force: true });
    }
  }
  await assertArchive(record);
  const extracted = join(CACHE, "trees", record.id);
  if (!existsSync(extracted)) {
    mkdirSync(extracted, { recursive: true });
    try {
      run("tar", [
        "-xzf",
        destination,
        "--strip-components=1",
        "-C",
        extracted,
      ]);
    } catch (error) {
      rmSync(extracted, { recursive: true, force: true });
      throw error;
    }
  }
}

async function main() {
  const mode = process.argv[2] ?? "check";
  const records = sources();
  if (mode === "check") {
    process.stdout.write(
      `Validated ${records.length} pinned source records.\n`,
    );
    return;
  }
  if (mode === "fetch") {
    for (const record of records) await fetchSource(record);
    process.stdout.write(
      `Fetched and verified ${records.length} pinned source trees.\n`,
    );
    return;
  }
  if (mode === "verify-cache") {
    for (const record of records) await assertArchive(record);
    process.stdout.write(
      `Verified ${records.length} cached source archives.\n`,
    );
    return;
  }
  throw new Error(`unknown source gate: ${mode}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) await main();
