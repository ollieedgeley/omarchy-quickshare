import { createHash } from "node:crypto";
import {
  createReadStream,
  createWriteStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { basename, join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

import { run } from "../../../tools/gates/lib/process.mjs";
import { toolPath } from "./paths.mjs";

const TAR_GZIP_PATTERN = /\.tar\.gz$/u;
const ZIP_PATTERN = /\.zip$/u;

export function archiveUrl(manifest, record) {
  const repository = manifest.repositories[record.source];
  if (!repository) {
    throw new Error(`unknown archive source ${record.source}`);
  }
  return new URL(record.archive, repository).href;
}

export function archivePath(paths, record) {
  return join(paths.archives, record.archive);
}

export async function fileSha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) {
    hash.update(chunk);
  }
  return hash.digest("hex");
}

export async function assertArchive(paths, record) {
  const path = archivePath(paths, record);
  if (!existsSync(path)) {
    throw new Error(`missing Android archive ${record.archive}`);
  }
  const { size } = statSync(path);
  const digest = await fileSha256(path);
  if (size !== record.size || digest !== record.sha256) {
    throw new Error(`Android archive verification failed for ${record.id}`);
  }
  return path;
}

async function writeResponse(response, destination) {
  if (!response.ok || !response.body) {
    throw new Error(`Android archive download returned ${response.status}`);
  }
  await pipeline(
    Readable.fromWeb(response.body),
    createWriteStream(destination),
  );
}

export async function fetchArchive(options) {
  const { fetchFunction = globalThis.fetch, manifest, paths, record } = options;
  mkdirSync(paths.archives, { recursive: true });
  const destination = archivePath(paths, record);
  if (existsSync(destination)) {
    return assertArchive(paths, record);
  }
  const temporary = mkdtempSync(join(paths.archives, ".download-"));
  const download = join(temporary, basename(destination));
  try {
    const response = await fetchFunction(archiveUrl(manifest, record));
    await writeResponse(response, download);
    const actualSize = statSync(download).size;
    const actualHash = await fileSha256(download);
    if (actualSize !== record.size || actualHash !== record.sha256) {
      throw new Error(`download verification failed for ${record.id}`);
    }
    renameSync(download, destination);
  } finally {
    rmSync(temporary, { force: true, recursive: true });
  }
  return destination;
}

function extractArguments(record, archive, destination) {
  if (TAR_GZIP_PATTERN.test(record.archive)) {
    return [
      "tar",
      ["-xzf", archive, "--strip-components=1", "-C", destination],
    ];
  }
  if (ZIP_PATTERN.test(record.archive)) {
    return ["unzip", ["-q", archive, "-d", destination]];
  }
  throw new Error(`unsupported Android archive ${record.archive}`);
}

export async function ensureExtracted(options) {
  const { manifest, paths, record, runCommand = run } = options;
  const destination = toolPath(paths, record);
  const marker = join(destination, ".archive-sha256");
  if (
    existsSync(marker) &&
    readFileSync(marker, "utf8").trim() === record.sha256
  ) {
    await assertArchive(paths, record);
    return destination;
  }
  rmSync(destination, { force: true, recursive: true });
  const archive = await fetchArchive({ manifest, paths, record });
  mkdirSync(destination, { recursive: true });
  try {
    const [command, args] = extractArguments(record, archive, destination);
    runCommand(command, args);
    writeFileSync(marker, `${record.sha256}\n`);
  } catch (error) {
    rmSync(destination, { force: true, recursive: true });
    throw error;
  }
  return destination;
}
