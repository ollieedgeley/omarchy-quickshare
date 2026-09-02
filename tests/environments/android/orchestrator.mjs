import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { run } from "../../../tools/gates/lib/process.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const PROBE_ROOT = join(DIRECTORY, "probe");
const DOCKERFILE = join(PROBE_ROOT, "Dockerfile.mobly");
const PROCESS_SHIM = join(PROBE_ROOT, "mobly-ps");
const REQUIREMENTS = join(PROBE_ROOT, "mobly-requirements.txt");
const BASE_IMAGE_DIRECTIVE_PATTERN =
  /^ARG BASE_IMAGE=(?<image>[^\n]+)\nFROM \$\{BASE_IMAGE\}\n/u;
const HASH_PATTERN = /--hash=sha256:[0-9a-f]{64}/gu;
const LINE_CONTINUATION_PATTERN = /\\\r?\n/gu;
const FINGERPRINT_LABEL = "io.omarchy-quickshare.environment";

function sourceFiles() {
  return {
    dockerfile: readFileSync(DOCKERFILE, "utf8"),
    processShim: readFileSync(PROCESS_SHIM, "utf8"),
    requirements: readFileSync(REQUIREMENTS, "utf8"),
  };
}

export function orchestratorFingerprint(manifest, sources = sourceFiles()) {
  return createHash("sha256")
    .update(JSON.stringify(manifest.probe.orchestrator))
    .update("\0")
    .update(sources.dockerfile)
    .update("\0")
    .update(sources.processShim)
    .update("\0")
    .update(sources.requirements)
    .digest("hex");
}

export function validateOrchestratorFiles(manifest, sources = sourceFiles()) {
  const { orchestrator } = manifest.probe;
  const logicalDockerfile = sources.dockerfile.replace(
    LINE_CONTINUATION_PATTERN,
    "",
  );
  const baseDirective = BASE_IMAGE_DIRECTIVE_PATTERN.exec(logicalDockerfile);
  if (baseDirective?.groups?.image !== orchestrator.baseImage) {
    throw new Error("Mobly container does not use the pinned base image");
  }
  if (!sources.dockerfile.includes(FINGERPRINT_LABEL)) {
    throw new Error("Mobly container lacks its environment fingerprint");
  }
  for (const [name, version] of Object.entries(orchestrator.dependencies)) {
    if (!sources.requirements.includes(`${name}==${version}`)) {
      throw new Error(`Mobly requirement ${name} is missing or unpinned`);
    }
  }
  const hashes = sources.requirements.match(HASH_PATTERN) ?? [];
  if (hashes.length !== Object.keys(orchestrator.dependencies).length) {
    throw new Error("Each Mobly requirement needs exactly one SHA-256 hash");
  }
  return sources;
}

export function provisionOrchestrator(manifest) {
  const sources = validateOrchestratorFiles(manifest);
  const fingerprint = orchestratorFingerprint(manifest, sources);
  run(process.env.DOCKER ?? "docker", [
    "build",
    "--file",
    DOCKERFILE,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${fingerprint}`,
    "--build-arg",
    `PYTHON_VERSION=${manifest.probe.orchestrator.python}`,
    "--tag",
    manifest.probe.orchestrator.image,
    PROBE_ROOT,
  ]);
}
