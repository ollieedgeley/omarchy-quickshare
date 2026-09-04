import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test, { afterEach } from "node:test";
import { fileURLToPath } from "node:url";

import { output, run } from "../../gates/lib/process.mjs";
import { createArchPackage } from "../arch-package.mjs";
import { createNativeRelease } from "../native-release.mjs";
import { hashFile } from "../source-build.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const temporaryDirectories = new Set();
const COMMIT_LENGTH = 40;
const SOURCE_COMMIT = "b".repeat(COMMIT_LENGTH);
const PKGVER_PATTERN = /^pkgver=0\.0\.0$/mu;
const ARCH_BINARY_PATTERN = /usr\/bin\/omarchy-quickshare/u;
const ARCH_SERVICE_PATTERN =
  /usr\/lib\/systemd\/user\/omarchy-quickshare\.service/u;
const ARCH_CONFIG_PATTERN = /usr\/share\/omarchy-quickshare\/config.toml/u;
const ARCH_LICENSE_PATTERN = /usr\/share\/licenses\/\$pkgname\/LICENSE/u;
const PACKAGE_EXEC_PATTERN =
  /^ExecStart=\/usr\/bin\/omarchy-quickshare --daemon$/mu;
const LOCAL_BIN_PATTERN = /%h\/\.local\/bin/u;
const PKGINFO_NAME_PATTERN = /^pkgname = omarchy-quickshare$/mu;
const PKGINFO_VERSION_PATTERN = /^pkgver = 0\.0\.0-1$/mu;
const PKGINFO_MEMBER = /^\.PKGINFO$/mu;
const RELEASE_ARCH_DEPENDS_PATTERN = /^release-arch: release-native(?: |$)/mu;

function temporaryDirectory() {
  const directory = mkdtempSync(join(tmpdir(), "quickshare-native-"));
  temporaryDirectories.add(directory);
  return directory;
}

afterEach(() => {
  for (const directory of temporaryDirectories) {
    rmSync(directory, { force: true, recursive: true });
  }
  temporaryDirectories.clear();
});

function preparedNativeRoot(root) {
  mkdirSync(join(root, "target", "release"), { recursive: true });
  mkdirSync(join(root, "packaging", "systemd"), { recursive: true });
  mkdirSync(join(root, "crates", "app"), { recursive: true });
  writeFileSync(join(root, "target", "release", "omarchy-quickshare"), "bin");
  writeFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.service"),
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare --daemon\n",
  );
  writeFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.toml"),
    "# default\n",
  );
  writeFileSync(join(root, "LICENSE"), "license\n");
  writeFileSync(
    join(root, "crates", "app", "Cargo.toml"),
    '[package]\nname = "omarchy-quickshare"\nversion = "0.0.0"\n',
  );
}

function runReleaseCommand(calls) {
  return (command, args, options) => {
    const call = { args, command };
    if (options) {
      call.options = options;
    }
    calls.push(call);
    if (command === "tar") {
      run(command, args, options);
    }
  };
}

test("native release strips the locked binary and records its checksum", () => {
  const root = temporaryDirectory();
  preparedNativeRoot(root);
  const destination = temporaryDirectory();
  const calls = [];
  const result = createNativeRelease({
    destination,
    root,
    runCommand: runReleaseCommand(calls),
    sourceCommit: SOURCE_COMMIT,
  });
  const checksums = readFileSync(join(destination, "SHA256SUMS"), "utf8");
  const config = readFileSync(
    join(destination, "omarchy-quickshare.toml"),
    "utf8",
  );
  const meta = JSON.parse(
    readFileSync(join(destination, "version.json"), "utf8"),
  );

  assert.equal(result.version, "0.0.0");
  assert.equal(
    result.sha256,
    hashFile(join(destination, "omarchy-quickshare")),
  );
  assert.equal(checksums, `${result.sha256}  omarchy-quickshare\n`);
  assert.deepEqual(meta, {
    controlProtocol: 2,
    sha256: result.sha256,
    sourceCommit: SOURCE_COMMIT,
    version: result.version,
  });
  assert.equal(config, "# default\n");
  assert.equal(calls[0].command, "cargo");
  assert.deepEqual(calls[0].args, [
    "build",
    "--release",
    "--locked",
    "--package",
    "omarchy-quickshare",
  ]);
  assert.equal(calls[1].command, "strip");
});

test("Arch package layout ships binary unit config and license", () => {
  const root = temporaryDirectory();
  preparedNativeRoot(root);
  const nativeDirectory = temporaryDirectory();
  const destination = temporaryDirectory();
  createNativeRelease({
    destination: nativeDirectory,
    root,
    runCommand: runReleaseCommand([]),
    sourceCommit: SOURCE_COMMIT,
  });
  const result = createArchPackage({
    destination,
    nativeDirectory,
    root,
    runCommand: runReleaseCommand([]),
  });
  const packaged = readFileSync(
    join(destination, "pkg", "usr", "bin", "omarchy-quickshare"),
    "utf8",
  );

  assert.equal(result.version, "0.0.0");
  assert.match(result.pkgbuild, PKGVER_PATTERN);
  assert.match(result.pkgbuild, ARCH_BINARY_PATTERN);
  assert.match(result.pkgbuild, ARCH_SERVICE_PATTERN);
  assert.match(result.pkgbuild, ARCH_CONFIG_PATTERN);
  assert.match(result.pkgbuild, ARCH_LICENSE_PATTERN);
  assert.equal(packaged, "bin");
  const unit = readFileSync(
    join(
      destination,
      "pkg",
      "usr",
      "lib",
      "systemd",
      "user",
      "omarchy-quickshare.service",
    ),
    "utf8",
  );
  const pkginfo = readFileSync(join(destination, "pkg", ".PKGINFO"), "utf8");
  const listing = output("tar", ["-tf", result.artifact]);

  assert.match(unit, PACKAGE_EXEC_PATTERN);
  assert.doesNotMatch(unit, LOCAL_BIN_PATTERN);
  assert.match(pkginfo, PKGINFO_NAME_PATTERN);
  assert.match(pkginfo, PKGINFO_VERSION_PATTERN);
  assert.match(listing, PKGINFO_MEMBER);
});

test("release-arch depends on the native artifact", () => {
  const makefile = readFileSync(join(ROOT, "Makefile"), "utf8");
  assert.match(makefile, RELEASE_ARCH_DEPENDS_PATTERN);
});
