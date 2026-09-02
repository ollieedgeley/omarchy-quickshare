import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { fingerprint, validate } from "./environment.mjs";
import { sha256FromOutput } from "./integrity.mjs";
import { addNearShareAttachmentId } from "./nearshare-source.mjs";
import { assertMatchingImageFingerprint } from "./prepared-images.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = join(DIRECTORY, "../../..");
const INTERNAL_NETWORK_PATTERN = /internal: true/u;
const NEARSHARE_ADDRESS_PATTERN = /172\.30\.45\.10/u;
const GOOGLE_ADDRESS_PATTERN = /172\.30\.45\.11/u;
const HOST_NETWORK_PATTERN = /network_mode: host/u;
const PIN_FINGERPRINT_EVENT_PATTERN = /event\("pin", fingerprint=/u;
const PIN_EVENT_PATTERN = /event\("pin", pin=/u;
const MISSING_SALT_PATTERN = /missing comparison salt/u;
const PIN_SALT_SIZE_PATTERN = /PIN_SALT_BYTES = 32/u;
const PIN_SALT_GENERATION_PATTERN = /randomBytes\(PIN_SALT_BYTES\)/u;
const PIN_SALT_VARIABLE_PATTERN = /QUICKSHARE_PIN_SALT/u;
const MISSING_SALT_GUARD_PATTERN = /if not salt/u;
const GOOGLE_TOKEN_FINGERPRINT_PATTERN = /token_fingerprint/u;
const GOOGLE_RAW_TOKEN_PATTERN = /token=" << \*metadata\.token/u;
const SHA256_MISMATCH_PATTERN = /SHA-256 mismatch/u;
const INVALID_SHA256_PATTERN = /invalid SHA-256/u;
const GOOGLE_TO_NEARSHARE_PATTERN = /"google-to-nearshare"/u;
const NEARSHARE_TO_GOOGLE_PATTERN = /"nearshare-to-google"/u;
const REPEATED_RUN_PATTERN = /repeatedGoogleToNearshare/u;
const RECEIVER_READY_PATTERN =
  /await waitForReceiver\(receiver, googleSends\)/u;
const FIXED_RECEIVER_DELAY_PATTERN =
  /await delay\(RECEIVER_START_DELAY_MS\);\n\s{2}const sender/u;
const PIN_EVIDENCE_PATTERN = /pins\.length < 2/u;
const PIN_MATCH_PATTERN = /new Set\(pins\)\.size !== 1/u;
const SHA256_DIGEST_LENGTH = 64;
const STALE_IMAGE_PATTERN = /prepared image .* is stale/u;
const METADATA_DRIFT_PATTERN = /file metadata construction changed/u;
const ATTACHMENT_ID_PATTERN = /fm\.id = payload_id/u;
const ATTACHMENT_TEST =
  "NearShare outbound metadata receives a stable nonzero attachment ID";

function source(name) {
  return readFileSync(join(DIRECTORY, name), "utf8");
}

test("Diverse LAN is a sealed bridge with both implementations", () => {
  const compose = source("compose.yaml");
  validate();
  assert.match(compose, INTERNAL_NETWORK_PATTERN);
  assert.match(compose, NEARSHARE_ADDRESS_PATTERN);
  assert.match(compose, GOOGLE_ADDRESS_PATTERN);
  assert.doesNotMatch(compose, HOST_NETWORK_PATTERN);
});

test("headless NearShare reports a PIN fingerprint without its PIN", () => {
  const driver = source("nearshare_driver.py");
  assert.match(driver, PIN_FINGERPRINT_EVENT_PATTERN);
  assert.doesNotMatch(driver, PIN_EVENT_PATTERN);
  assert.match(driver, MISSING_SALT_PATTERN);
  assert.equal(
    fingerprint(Buffer.from("bytes")),
    fingerprint(Buffer.from("bytes")),
  );
});

test("Google CLI fingerprints its confirmation token", () => {
  const patch = readFileSync(
    join(ROOT, "tests/environments/nearby-linux/cli-actions.patch"),
    "utf8",
  );
  assert.match(patch, GOOGLE_TOKEN_FINGERPRINT_PATTERN);
  assert.doesNotMatch(patch, GOOGLE_RAW_TOKEN_PATTERN);
  assert.ok(
    patch.indexOf("TokenToFourDigitString") <
      patch.indexOf("event=paired-key-token") &&
      patch.indexOf("event=paired-key-token") <
        patch.indexOf("key_verification_runner_ ="),
  );
});

test("PIN comparison uses a per-run salt and fails closed without one", () => {
  const environment = source("environment.mjs");
  const driver = source("nearshare_driver.py");
  assert.match(environment, PIN_SALT_SIZE_PATTERN);
  assert.match(environment, PIN_SALT_GENERATION_PATTERN);
  assert.match(environment, PIN_SALT_VARIABLE_PATTERN);
  assert.match(driver, MISSING_SALT_GUARD_PATTERN);
  assert.match(driver, MISSING_SALT_PATTERN);
});

test("in-container SHA-256 output accepts only a complete digest", () => {
  const digest = "a".repeat(SHA256_DIGEST_LENGTH);
  assert.equal(
    sha256FromOutput(`${digest} */cases/received/file`, "google"),
    digest,
  );
  assert.throws(
    () => sha256FromOutput("not-a-digest", "nearshare"),
    INVALID_SHA256_PATTERN,
  );
});

test("prepared image fingerprint rejects a mutable stale tag", () => {
  assert.throws(
    () =>
      assertMatchingImageFingerprint({
        actual: "old-image",
        expected: "current-image",
        image: "omarchy-quickshare/nearshare-peer:current",
        provision: "nearshare-provision",
      }),
    STALE_IMAGE_PATTERN,
  );
});

test(ATTACHMENT_TEST, () => {
  const before =
    "            fm.name = path.name\n" +
    "            fm.payload_id = payload_id\n";
  const after = addNearShareAttachmentId(before);
  assert.match(after, ATTACHMENT_ID_PATTERN);
  assert.throws(() => addNearShareAttachmentId(after), METADATA_DRIFT_PATTERN);
});

test("interop gate covers both roles and a clean repeated control", () => {
  const environment = source("environment.mjs");
  assert.match(environment, GOOGLE_TO_NEARSHARE_PATTERN);
  assert.match(environment, NEARSHARE_TO_GOOGLE_PATTERN);
  assert.match(environment, REPEATED_RUN_PATTERN);
  assert.match(environment, PIN_EVIDENCE_PATTERN);
  assert.match(environment, PIN_MATCH_PATTERN);
  assert.match(environment, SHA256_MISMATCH_PATTERN);
  assert.match(environment, RECEIVER_READY_PATTERN);
  assert.doesNotMatch(environment, FIXED_RECEIVER_DELAY_PATTERN);
});
