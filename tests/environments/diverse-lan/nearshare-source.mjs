import { cpSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const METADATA_WITHOUT_ID =
  "            fm.name = path.name\n" +
  "            fm.payload_id = payload_id\n";
const METADATA_WITH_ID =
  "            fm.name = path.name\n" +
  "            fm.id = payload_id\n" +
  "            fm.payload_id = payload_id\n";

export function addNearShareAttachmentId(source) {
  const first = source.indexOf(METADATA_WITHOUT_ID);
  const last = source.lastIndexOf(METADATA_WITHOUT_ID);
  if (first < 0 || first !== last) {
    throw new Error("NearShare file metadata construction changed");
  }
  return source.replace(METADATA_WITHOUT_ID, METADATA_WITH_ID);
}

export function prepareNearShareSource(upstream, destination) {
  cpSync(upstream, destination, {
    preserveTimestamps: true,
    recursive: true,
  });
  const connection = join(destination, "nearshare", "core", "connection.py");
  const updated = addNearShareAttachmentId(readFileSync(connection, "utf8"));
  writeFileSync(connection, updated);
}
