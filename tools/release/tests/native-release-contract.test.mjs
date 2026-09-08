import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, test } from "vitest";

import { output, run } from "../../gates/lib/process.mjs";
import { createArchPackage } from "../arch-package.mjs";
import { createNativeRelease } from "../native-release.mjs";
import { hashFile } from "../source-build.mjs";

const temporaryDirectories = new Set();
const COMMIT_LENGTH = 40;
const CONTROL_PROTOCOL = 7;
const SOURCE_COMMIT = "b".repeat(COMMIT_LENGTH);
const ARCH_BINARY_PATTERN = /^usr\/bin\/omarchy-quickshare$/mu;
const ARCH_DAEMON_PATTERN = /^usr\/bin\/omarchy-quickshare-daemon$/mu;
const ARCH_SERVICE_PATTERN =
  /usr\/lib\/systemd\/user\/omarchy-quickshare\.service/u;
const ARCH_CONFIG_PATTERN = /usr\/share\/omarchy-quickshare\/config.toml/u;
const ARCH_LICENSE_PATTERN =
  /usr\/share\/licenses\/omarchy-quickshare\/LICENSE/u;
const PACKAGE_EXEC_PATTERN =
  /^ExecStart=\/usr\/bin\/omarchy-quickshare-daemon$/mu;
const LOCAL_BIN_PATTERN = /%h\/\.local\/bin/u;
const PKGINFO_NAME_PATTERN = /^pkgname = omarchy-quickshare$/mu;
const PKGINFO_VERSION_PATTERN = /^pkgver = 0\.0\.0-1$/mu;
const PKGINFO_MEMBER = /^\.PKGINFO$/mu;

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
  mkdirSync(join(root, "crates", "cli"), { recursive: true });
  writeFileSync(join(root, "target", "release", "omarchy-quickshare"), "cli");
  writeFileSync(
    join(root, "target", "release", "omarchy-quickshare-daemon"),
    "daemon",
  );
  writeFileSync(
    join(root, "packaging", "systemd", "omarchy-quickshare.service"),
    "[Service]\nExecStart=%h/.local/bin/omarchy-quickshare-daemon\n",
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
  writeFileSync(
    join(root, "crates", "cli", "Cargo.toml"),
    '[package]\nname = "omarchy-quickshare-cli"\nversion = "0.0.0"\n',
  );
}

function runReleaseCommand() {
  return (command, args, options) => {
    if (args[0] === "protocol-version" || args[0] === "--protocol-version") {
      return { stdout: String(CONTROL_PROTOCOL) };
    }
    if (command === "tar") {
      run(command, args, options);
    }
    return null;
  };
}

test("native release checksums fixture bytes with simulated builds", () => {
  const root = temporaryDirectory();
  preparedNativeRoot(root);
  const destination = temporaryDirectory();
  const result = createNativeRelease({
    destination,
    root,
    runCommand: runReleaseCommand(),
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
  assert.equal(result.controlProtocol, CONTROL_PROTOCOL);
  const components = {
    cli: {
      file: "omarchy-quickshare",
      sha256: hashFile(join(destination, "omarchy-quickshare")),
    },
    daemon: {
      file: "omarchy-quickshare-daemon",
      sha256: hashFile(join(destination, "omarchy-quickshare-daemon")),
    },
  };
  for (const role of ["cli", "daemon"]) {
    const file = join(destination, components[role].file);
    assert.equal(readFileSync(file, "utf8"), role);
  }
  assert.deepEqual(result.components, components);
  assert.equal(
    checksums,
    `${components.cli.sha256}  omarchy-quickshare\n` +
      `${components.daemon.sha256}  omarchy-quickshare-daemon\n`,
  );
  assert.deepEqual(meta, {
    components,
    controlProtocol: CONTROL_PROTOCOL,
    schemaVersion: 2,
    sourceCommit: SOURCE_COMMIT,
    version: result.version,
  });
  assert.equal(config, "# default\n");
});

test("Arch archive ships both fixture binaries unit config and license", () => {
  const root = temporaryDirectory();
  preparedNativeRoot(root);
  const nativeDirectory = temporaryDirectory();
  const destination = temporaryDirectory();
  createNativeRelease({
    destination: nativeDirectory,
    root,
    runCommand: runReleaseCommand(),
    sourceCommit: SOURCE_COMMIT,
  });
  const result = createArchPackage({
    destination,
    nativeDirectory,
    root,
    runCommand: runReleaseCommand(),
  });
  assert.equal(result.version, "0.0.0");
  assert.equal(
    output("tar", ["-xOf", result.artifact, "usr/bin/omarchy-quickshare"]),
    "cli",
  );
  assert.equal(
    output("tar", [
      "-xOf",
      result.artifact,
      "usr/bin/omarchy-quickshare-daemon",
    ]),
    "daemon",
  );
  const unit = output("tar", [
    "-xOf",
    result.artifact,
    "usr/lib/systemd/user/omarchy-quickshare.service",
  ]);
  const pkginfo = readFileSync(join(destination, "pkg", ".PKGINFO"), "utf8");
  const listing = output("tar", ["-tf", result.artifact]);

  assert.match(unit, PACKAGE_EXEC_PATTERN);
  assert.doesNotMatch(unit, LOCAL_BIN_PATTERN);
  assert.match(pkginfo, PKGINFO_NAME_PATTERN);
  assert.match(pkginfo, PKGINFO_VERSION_PATTERN);
  assert.match(listing, PKGINFO_MEMBER);
  assert.match(listing, ARCH_BINARY_PATTERN);
  assert.match(listing, ARCH_DAEMON_PATTERN);
  assert.match(listing, ARCH_SERVICE_PATTERN);
  assert.match(listing, ARCH_CONFIG_PATTERN);
  assert.match(listing, ARCH_LICENSE_PATTERN);
});
