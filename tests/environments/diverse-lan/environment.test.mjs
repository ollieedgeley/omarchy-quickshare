import assert from "node:assert/strict";
import { test } from "vitest";

import { sha256FromOutput } from "./integrity.mjs";
import { addNearShareAttachmentId } from "./nearshare-source.mjs";
import { assertMatchingImageFingerprint } from "./prepared-images.mjs";

const INVALID_SHA256_PATTERN = /invalid SHA-256/u;
const SHA256_DIGEST_LENGTH = 64;
const STALE_IMAGE_PATTERN = /prepared image .* is stale/u;
const METADATA_DRIFT_PATTERN = /file metadata construction changed/u;
const ATTACHMENT_ID_PATTERN = /fm\.id = payload_id/u;
const ATTACHMENT_TEST =
  "NearShare source adaptation inserts an attachment ID and rejects drift";

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
