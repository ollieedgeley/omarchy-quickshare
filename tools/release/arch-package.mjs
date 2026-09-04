import {
  chmodSync,
  copyFileSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../gates/lib/process.mjs";
import { readAppVersion } from "./source-allowlist.mjs";
import { hashFile } from "./source-build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const NATIVE = join(ROOT, "dist", "native");
const DESTINATION = join(ROOT, "dist", "arch");
const PKGBUILD_TEMPLATE = join(ROOT, "packaging", "arch", "PKGBUILD");
const BINARY_NAME = "omarchy-quickshare";
const EXECUTABLE_MODE = 0o755;
const PKGREL = 1;
const MILLISECONDS_PER_SECOND = 1000;
const PACKAGE_EXEC = "ExecStart=/usr/bin/omarchy-quickshare daemon";
const EXEC_START_PATTERN = /^ExecStart=.*$/mu;
const FILES = [
  BINARY_NAME,
  `${BINARY_NAME}.service`,
  `${BINARY_NAME}.toml`,
  "LICENSE",
];
const PKGVER_PATTERN = /^pkgver=.*/mu;
const SHA256SUMS_PATTERN = /^sha256sums=.*/mu;

function renderPkgbuild(template, version, sums) {
  const quoted = sums.map((sum) => `'${sum}'`).join(" ");
  return template
    .replace(PKGVER_PATTERN, `pkgver=${version}`)
    .replace(SHA256SUMS_PATTERN, `sha256sums=(${quoted})`);
}

function renderPackageUnit(source) {
  if (!EXEC_START_PATTERN.test(source)) {
    throw new Error("package unit is missing ExecStart");
  }
  return source.replace(EXEC_START_PATTERN, PACKAGE_EXEC);
}

function stageSources(nativeDirectory, destination) {
  mkdirSync(destination, { recursive: true });
  for (const file of FILES) {
    copyFileSync(join(nativeDirectory, file), join(destination, file));
  }
  chmodSync(join(destination, BINARY_NAME), EXECUTABLE_MODE);
  const unitPath = join(destination, `${BINARY_NAME}.service`);
  writeFileSync(unitPath, renderPackageUnit(readFileSync(unitPath, "utf8")));
}

function installedSize(root) {
  let size = 0;
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    const metadata = lstatSync(current);
    if (metadata.isDirectory()) {
      for (const entry of readdirSync(current)) {
        stack.push(join(current, entry));
      }
    } else if (metadata.isFile()) {
      size += metadata.size;
    }
  }
  return size;
}

function writePkginfo(pkgdir, version) {
  const size = installedSize(join(pkgdir, "usr"));
  const builddate = Math.floor(Date.now() / MILLISECONDS_PER_SECOND);
  writeFileSync(
    join(pkgdir, ".PKGINFO"),
    [
      `pkgname = ${BINARY_NAME}`,
      `pkgbase = ${BINARY_NAME}`,
      `pkgver = ${version}-${PKGREL}`,
      "pkgdesc = Account-free Quick Share endpoint for Omarchy",
      `builddate = ${builddate}`,
      "packager = omarchy-quickshare",
      `size = ${size}`,
      "arch = x86_64",
      "license = Apache-2.0",
      "",
    ].join("\n"),
  );
}

function writePackageTree(destination) {
  const pkgdir = join(destination, "pkg");
  mkdirSync(join(pkgdir, "usr", "bin"), { recursive: true });
  mkdirSync(join(pkgdir, "usr", "lib", "systemd", "user"), {
    recursive: true,
  });
  mkdirSync(join(pkgdir, "usr", "share", BINARY_NAME), { recursive: true });
  mkdirSync(join(pkgdir, "usr", "share", "licenses", BINARY_NAME), {
    recursive: true,
  });
  copyFileSync(
    join(destination, BINARY_NAME),
    join(pkgdir, "usr", "bin", BINARY_NAME),
  );
  copyFileSync(
    join(destination, `${BINARY_NAME}.service`),
    join(pkgdir, "usr", "lib", "systemd", "user", `${BINARY_NAME}.service`),
  );
  copyFileSync(
    join(destination, `${BINARY_NAME}.toml`),
    join(pkgdir, "usr", "share", BINARY_NAME, "config.toml"),
  );
  copyFileSync(
    join(destination, "LICENSE"),
    join(pkgdir, "usr", "share", "licenses", BINARY_NAME, "LICENSE"),
  );
  return pkgdir;
}

export function createArchPackage({
  destination,
  nativeDirectory = NATIVE,
  root = ROOT,
  runCommand = run,
} = {}) {
  const version = readAppVersion(root);
  stageSources(nativeDirectory, destination);
  const pkgdir = writePackageTree(destination);
  writePkginfo(pkgdir, version);
  const sums = FILES.map((file) => hashFile(join(destination, file)));
  const pkgbuild = renderPkgbuild(
    readFileSync(PKGBUILD_TEMPLATE, "utf8"),
    version,
    sums,
  );
  writeFileSync(join(destination, "PKGBUILD"), pkgbuild);
  const artifact = join(
    destination,
    `${BINARY_NAME}-${version}-${PKGREL}-x86_64.pkg.tar.zst`,
  );
  runCommand("tar", [
    "-C",
    pkgdir,
    "--zstd",
    "-cf",
    artifact,
    ".PKGINFO",
    "usr",
  ]);
  return { artifact, pkgbuild, version };
}

function main() {
  if (resolve(DESTINATION) !== DESTINATION) {
    throw new Error("refusing to clean an unexpected release path");
  }
  rmSync(DESTINATION, { force: true, recursive: true });
  const result = createArchPackage({ destination: DESTINATION });
  const sourceCommit = output("git", ["rev-parse", "HEAD"]);
  process.stdout.write(
    `Arch package ${result.version} from ${sourceCommit}.\n`,
  );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
