import { readFileSync } from "node:fs";
import { join } from "node:path";

import { output, run } from "../../../tools/gates/lib/process.mjs";
const { environmentFingerprint: nearShareFingerprint } = await import(
  new URL("../nearshare/environment.mjs", import.meta.url)
);

const NEARBY_VALIDATION_PATTERN =
  /Validated Nearby Linux environment (?<fingerprint>[a-f0-9]{64})\./u;
const IMAGE_FINGERPRINT_LABEL = "org.omarchy-quickshare.fingerprint";
const NEARSHARE_FINGERPRINT_LABEL = "io.omarchy-quickshare.environment";

function imageFingerprint(image, label) {
  const { status, stdout } = run(
    process.env.DOCKER ?? "docker",
    [
      "image",
      "inspect",
      "--format",
      `{{index .Config.Labels "${label}"}}`,
      image,
    ],
    { allowFailure: true, capture: true, quiet: true },
  );
  if (status !== 0) {
    return "";
  }
  return stdout.trim();
}

export function assertMatchingImageFingerprint({
  actual,
  expected,
  image,
  provision,
}) {
  if (actual !== expected) {
    throw new Error(`prepared image ${image} is stale; run make ${provision}`);
  }
}

function nearbyLinuxFingerprint(root) {
  const result = output(process.execPath, [
    join(root, "tests/environments/nearby-linux/environment.mjs"),
    "validate",
  ]);
  const expectedFingerprint = result.match(NEARBY_VALIDATION_PATTERN)?.groups
    .fingerprint;
  if (!expectedFingerprint) {
    throw new Error("Nearby Linux validation did not report its fingerprint");
  }
  return expectedFingerprint;
}

export function assertPreparedImages({
  googleImage,
  nearShareImage,
  nearShareManifest,
  root,
}) {
  assertMatchingImageFingerprint({
    actual: imageFingerprint(googleImage, IMAGE_FINGERPRINT_LABEL),
    expected: nearbyLinuxFingerprint(root),
    image: googleImage,
    provision: "nearby-linux-provision",
  });
  const dockerfile = readFileSync(
    join(root, "tests/environments/nearshare/Dockerfile.toolchain"),
    "utf8",
  );
  assertMatchingImageFingerprint({
    actual: imageFingerprint(nearShareImage, NEARSHARE_FINGERPRINT_LABEL),
    expected: nearShareFingerprint(nearShareManifest, dockerfile),
    image: nearShareImage,
    provision: "nearshare-provision",
  });
}
