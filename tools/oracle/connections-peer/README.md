# Connections reference peer

This test-only executable wraps the pinned Google Nearby Connections v3
production implementation. It does not implement a Connections frame or
medium.

Copy `connections_peer.cc` to `connections/file_share/` in the prepared pinned
Nearby checkout and add the `cc_binary` declaration from `BUILD.bazel` to that
checkout's `connections/file_share/BUILD` (it already loads `cc_binary`). Then
compile it with:

```sh
bazel build //connections/file_share:connections_peer
```

The LAN control is deterministic:

```sh
bazel-bin/connections/file_share/connections_peer --advertise \
  --initial-medium=wifi_lan --upgrade-medium=wifi_lan --endpoint-name=peer-a
```

`--initial-medium` configures advertising, discovery, and the request's
`ConnectionOptions.allowed`; `--upgrade-medium` configures only advertising's
upgrade candidates. `--auto-upgrade` enables Google's automatic upgrade
selection. It is independent of `--initiate-upgrade-on-connect`, which calls
`Core::InitiateBandwidthUpgradeV3` when a connection succeeds. Both peers may
use that local control to create a simultaneous-proposal case.

A `ready` event records both controls. `upgrade-request` records the local
request, while `upgrade-result` records the API result (that the request was
triggered, not a completed cutover). `bandwidth-changed` is emitted only from
Google's `ConnectionListener::bandwidth_changed_cb`; it records the prior
observed `old_medium` and the callback's actual `new_medium`. The initial
medium is the forced initial control until Google reports a later change.
`--decision=reject` forces connection rejection; the default accepts.

Use exactly one outgoing-payload control: `--send-file=PATH` sends that file
through Google's `Payload` and `--send-text=TEXT` sends those exact UTF-8 bytes.
The peer emits `payload-send` with its generated ID and expected byte count,
then `payload-progress` and `payload-terminal` with the Google-reported byte
counts and terminal status. Incoming file payloads add `received_file` to the
terminal event so the environment can calculate SHA-256 from its case mount;
incoming bytes report their byte count without logging their contents.

`--cancel-on-progress`, `--disconnect-on-connect`,
`--disconnect-on-progress`, and `--disconnect-on-bandwidth-changed` are
one-shot deterministic controls. The last disconnects immediately after
`bandwidth_changed_cb`, providing a new-channel-loss case. They call the Google
v3 cancellation or disconnect API at the named callback boundary. A Google
`disconnected_cb` emits the matching `disconnected` event.

The v3 request API has no independent client-side upgrade selector or
upgrade-rejection API. The peer never substitutes upgrade media for
`ConnectionOptions.allowed`; candidate disappearance, post-establishment
new-channel loss, and rejection/failure must be injected by the selected
medium environment. A successful request API result is not evidence that the
upgrade completed; use `bandwidth-changed` and payload evidence.
