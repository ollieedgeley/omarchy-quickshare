import {
  copyFileSync,
  cpSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { gunzipSync } from "node:zlib";

import { patchGoogleOverlay } from "./google-overlay.mjs";

function assertCachePath(cacheRoot, path) {
  const expectedPrefix = `${cacheRoot}/`;
  if (!resolve(path).startsWith(expectedPrefix)) {
    throw new Error(`refusing unsafe test-environment path: ${path}`);
  }
}

function replaceExpected(source, [before, after], count = 1) {
  const occurrences = source.split(before).length - 1;
  if (occurrences !== count) {
    throw new Error(
      `expected ${count} Google overlay occurrence(s), ` +
        `found ${occurrences}: ${before.trim()}`,
    );
  }
  return source.replaceAll(before, after);
}

function prepareGoogleLinuxOverlay(workspace) {
  const googlePath = (...parts) => join(workspace, ...parts);
  const buildPath = googlePath(
    "internal",
    "platform",
    "implementation",
    "g3",
    "BUILD",
  );
  let build = readFileSync(buildPath, "utf8");
  for (const [line, count] of [
    ['        "webrtc.cc",\n', 1],
    ['        "webrtc.h",\n', 1],
    ['        "webrtc_platform.cc",\n', 1],
    ['        "//internal/platform/implementation:webrtc_platform",\n', 2],
    [
      '        "//third_party/webrtc/files/stable/webrtc/api:' +
        'create_modular_peer_connection_factory",\n',
      1,
    ],
    [
      '        "//third_party/webrtc/files/stable/webrtc/api:' +
        'peer_connection_interface",\n',
      1,
    ],
    [
      '        "//third_party/webrtc/files/stable/webrtc/api:scoped_refptr",\n',
      1,
    ],
    [
      '        "//third_party/webrtc/files/stable/webrtc/rtc_base:checks",\n',
      1,
    ],
    [
      '        "//third_party/webrtc/files/stable/webrtc/rtc_base:' +
        'threading",\n',
      1,
    ],
  ]) {
    build = replaceExpected(build, [line, ""], count);
  }
  build = replaceExpected(build, [
    '        "@com_google_protobuf//json",\n',
    '        "@com_google_protobuf//:json",\n',
  ]);
  writeFileSync(buildPath, build);
  patchGoogleOverlay(googlePath, replaceExpected);
}

function configureGoogleWorkspace({ fixtureGenerator, source, workspace }) {
  const modulePath = join(workspace, "MODULE.bazel");
  const module = readFileSync(modulePath, "utf8");
  writeFileSync(
    modulePath,
    replaceExpected(module, [
      'bazel_dep(name = "rules_rust", version = "0.68.1")\n',
      "bazel_dep(\n" +
        '    name = "rules_rust",\n' +
        '    version = "0.68.1",\n' +
        '    repo_name = "rules_rust",\n' +
        ")\n",
    ]),
  );
  cpSync(
    fixtureGenerator,
    join(workspace, "tools", "quickshare_fixture_generator"),
    { recursive: true, preserveTimestamps: true },
  );
  const generatorFake = join(
    workspace,
    "tools",
    "quickshare_fixture_generator",
    "external",
    "sharing",
  );
  mkdirSync(generatorFake, { recursive: true });
  for (const name of [
    "fake_nearby_connections_manager.cc",
    "fake_nearby_connections_manager.h",
  ]) {
    copyFileSync(join(source, "sharing", name), join(generatorFake, name));
  }
}

function copyGoogleOverlays(directory, workspace) {
  cpSync(
    join(directory, "overlays", "gloop"),
    join(workspace, "third_party", "gloop"),
    { recursive: true, preserveTimestamps: true },
  );
  cpSync(
    join(directory, "overlays", "webrtc"),
    join(workspace, "third_party", "webrtc", "files", "stable", "webrtc"),
    { recursive: true, preserveTimestamps: true },
  );
  prepareGoogleLinuxOverlay(workspace);
}

function copyGoogleSource(preparation) {
  const { cacheRoot, directory, sourceTrees, workspace } = preparation;
  assertCachePath(cacheRoot, workspace);
  const source = join(sourceTrees, "google-nearby");
  if (!readFileSync(join(source, "MODULE.bazel"), "utf8")) {
    throw new Error(
      "Google Nearby source is missing; run `make sources-fetch`",
    );
  }
  rmSync(workspace, { recursive: true, force: true });
  mkdirSync(dirname(workspace), { recursive: true });
  cpSync(source, workspace, { recursive: true, preserveTimestamps: true });
  configureGoogleWorkspace({
    fixtureGenerator: preparation.fixtureGenerator,
    source,
    workspace,
  });
  copyGoogleOverlays(directory, workspace);
}

function prepareSimpleOverride(preparation, sourceName, buildFile) {
  const { directory, overrides, sourceTrees } = preparation;
  const destination = join(overrides, sourceName);
  cpSync(join(sourceTrees, sourceName), destination, {
    preserveTimestamps: true,
    recursive: true,
  });
  copyFileSync(
    join(directory, "overlays", buildFile),
    join(destination, "BUILD.bazel"),
  );
  writeFileSync(join(destination, "WORKSPACE"), "");
}

function prepareOverrides(preparation) {
  const { directory, overrides, sourceTrees } = preparation;
  rmSync(overrides, { recursive: true, force: true });
  mkdirSync(overrides, { recursive: true });
  for (const [sourceName, buildFile] of [
    ["smhasher", "smhasher.BUILD.bazel"],
    ["nlohmann-json", "nlohmann-json.BUILD.bazel"],
  ]) {
    prepareSimpleOverride(preparation, sourceName, buildFile);
  }
  const nisaba = join(overrides, "nisaba");
  cpSync(join(sourceTrees, "nisaba"), nisaba, {
    preserveTimestamps: true,
    recursive: true,
  });
  copyFileSync(
    join(directory, "overlays", "nisaba-port.BUILD.bazel"),
    join(nisaba, "nisaba", "port", "BUILD.bazel"),
  );
  copyFileSync(
    join(directory, "overlays", "nisaba-thread-pool.h"),
    join(nisaba, "nisaba", "port", "thread_pool.h"),
  );
  writeFileSync(join(nisaba, "WORKSPACE"), "");
}

function writeReferenceMetadata({
  directory,
  fingerprint,
  manifest,
  validateReferenceLock,
  workspace,
}) {
  const lockArchive = join(directory, manifest.reference.lockFile);
  const compressed = readFileSync(lockArchive);
  validateReferenceLock(manifest, compressed);
  writeFileSync(join(workspace, "MODULE.bazel.lock"), gunzipSync(compressed));
  writeFileSync(
    join(workspace, ".quickshare-reference.json"),
    `${JSON.stringify(
      { fingerprint, sources: manifest.reference.sources },
      null,
      2,
    )}\n`,
  );
}

export function prepareOracleReference({
  cacheRoot,
  directory,
  fingerprint,
  fixtureGenerator,
  manifest,
  overrides,
  sourceTrees,
  validateReferenceLock,
  workspace,
}) {
  const preparation = {
    cacheRoot,
    directory,
    fixtureGenerator,
    sourceTrees,
    workspace,
  };
  copyGoogleSource(preparation);
  prepareOverrides({ ...preparation, overrides });
  writeReferenceMetadata({
    directory,
    fingerprint,
    manifest,
    validateReferenceLock,
    workspace,
  });
}
