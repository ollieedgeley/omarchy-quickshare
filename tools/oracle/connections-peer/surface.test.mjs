import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const source = [
  "connections_peer.cc",
  "connections_peer.h",
  "connections_peer_main.cc",
  "connections_peer_options.cc",
]
  .map((name) => {
    const path = fileURLToPath(new URL(name, import.meta.url));
    return readFileSync(path, "utf8");
  })
  .join("\n");
const INITIAL_ALLOWED_PATTERN = /: options_\.initial_mediums/u;
const UPGRADE_OPTIONS_PATTERN =
  /options\.upgrade_mediums = options_\.upgrade_mediums/u;
const BANDWIDTH_CALLBACK_PATTERN = /bandwidth_changed_cb/u;
const OLD_MEDIUM_PATTERN = /old_medium/u;
const NEW_MEDIUM_PATTERN = /new_medium/u;
const UPGRADE_ALLOWED_PATTERN = /\? options_\.upgrade_mediums/u;
const AUTO_UPGRADE_PATTERN = /--auto-upgrade/u;
const LOCAL_UPGRADE_PATTERN = /--initiate-upgrade-on-connect/u;
const LOCAL_UPGRADE_CALL_PATTERN = /InitiateBandwidthUpgradeV3/u;
const UPGRADE_REQUEST_PATTERN = /upgrade-request/u;
const UPGRADE_RESULT_PATTERN = /upgrade-result/u;
const NEW_CHANNEL_LOSS_PATTERN = /--disconnect-on-bandwidth-changed/u;
const PROPOSAL_REQUIRES_UPGRADE_PATTERN =
  /!proposes_upgrade \|\| options->upgrade_mediums\.Any\(true\)/u;
const SEND_FILE_PATTERN = /--send-file/u;
const SEND_TEXT_PATTERN = /--send-text/u;
const PAYLOAD_SEND_PATTERN = /SendPayloadV3/u;
const PAYLOAD_TERMINAL_PATTERN = /payload-terminal/u;
const PROGRESS_PATTERN = /payload_progress_cb/u;
const DISCONNECTED_PATTERN = /disconnected_cb/u;
const CANCELLATION_PATTERN = /CancelPayloadV3/u;
const DISCONNECT_PATTERN = /DisconnectFromDeviceV3/u;

assert.match(source, INITIAL_ALLOWED_PATTERN);
assert.match(source, UPGRADE_OPTIONS_PATTERN);
assert.match(source, BANDWIDTH_CALLBACK_PATTERN);
assert.match(source, OLD_MEDIUM_PATTERN);
assert.match(source, NEW_MEDIUM_PATTERN);
assert.match(source, UPGRADE_ALLOWED_PATTERN);
assert.match(source, AUTO_UPGRADE_PATTERN);
assert.match(source, LOCAL_UPGRADE_PATTERN);
assert.match(source, LOCAL_UPGRADE_CALL_PATTERN);
assert.match(source, UPGRADE_REQUEST_PATTERN);
assert.match(source, UPGRADE_RESULT_PATTERN);
assert.match(source, NEW_CHANNEL_LOSS_PATTERN);
assert.match(source, PROPOSAL_REQUIRES_UPGRADE_PATTERN);
assert.match(source, SEND_FILE_PATTERN);
assert.match(source, SEND_TEXT_PATTERN);
assert.match(source, PAYLOAD_SEND_PATTERN);
assert.match(source, PAYLOAD_TERMINAL_PATTERN);
assert.match(source, PROGRESS_PATTERN);
assert.match(source, DISCONNECTED_PATTERN);
assert.match(source, CANCELLATION_PATTERN);
assert.match(source, DISCONNECT_PATTERN);
