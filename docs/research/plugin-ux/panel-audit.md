# Panel, discovery, and draft audit

Research date: 2026-09-06. Scope is planning for user points 1, 3, and 4. No application changes, tests, discovery, clipboard access, installation, or desktop actions were performed by this audit.

## Evidence and limits

CodeGraph was queried first for `QuickShare plugin QML Panel discovery cancel paste clipboard selectedDevice state subscription`, then exact plugin paths, then `stop_discovery StopDiscovery discovery start cli dispatch daemon runtime control`. It returned Rust snapshot and worker ownership but reported no indexed match for the QML files. QML and omitted Rust sections were therefore read directly. Changed-index warnings for CLI dispatch and daemon lifecycle were handled by reading their current source.

Repository references below describe the working tree on the research date, not a release or the user's installed copy. Read policies were `CONTEXT.md`, `AGENTS.md`, `docs/architecture/project-structure.md`, the feasibility documents, `docs/connection-mocking-tools.md` local-control and transfer seams, and `docs/development-workflow.md` behavior-development policy. This is repository-specific research; no external platform behavior is asserted.

The F2.1 capture-first regression now runs real `BarWidget` and `StatusProbe`
with the real CLI and an isolated simulated daemon. After capturing A and
changing the fake clipboard to B, explicit selection admits A to the chosen
peer. This replaces the capture-reread finding below for the implemented
capture-first path; it does not establish browse cleanup or native keyboard
behavior.

The reported inability to stop searching and capture content before selecting a device is accepted as real. The full runtime cause is not proven. Main owns runtime evidence and reported that native desktop inspection could not capture this environment. This note does not claim a visual audit or physical-peer test.

## Current flow

| Owner                                                                          | Proven behavior                                                                                                                                                                                  |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `packaging/omarchy-plugin/BarWidget.qml:35-42`, `open`, `close`                | `open` already attempts `status.discover()` every time it is called. `close` only hides the popup; it does not stop discovery or cancel a share.                                                 |
| `BarWidget.qml:102-107`, `paste`                                               | An open panel captures a nonempty value locally. A closed panel submits directly to the daemon and tracks whether it must open a picker.                                                         |
| `BarWidget.qml:66-100`, `captureClipboard`, `finishClipboard`, `readClipboard` | Captured content lives in `clipboardPreview` plus `pasteLatch`. Clipboard capture uses `wl-paste` URI-list first, then generic text. No editor exists.                                           |
| `BarWidget.qml:115-145`                                                        | Standard Paste works while the popup is open and starts clipboard reading independently of `status.actionBusy`.                                                                                  |
| `BarWidget.qml:319-321`, `onPeerSelected`                                      | A pending daemon share uses `share select`. With no active share, choosing a device reads the clipboard again and sends that new value, not the captured preview.                                |
| `SharePanel.qml:37-50`, `viewState`                                            | No active share plus searching, timeout, or any peer selects `peer_choice`; otherwise it selects `idle`. A `waiting_for_peer` share always selects `peer_choice`, even after stopping discovery. |
| `SharePanel.qml:297-401`                                                       | `idle` is receive visibility only. `peer_choice` is a separate device picker. Receive controls are hidden during discovery. The attachment badge exists in the picker, not the idle section.     |
| `SharePanel.qml:163-190`, `cancel`, `toggleDiscovery`                          | Stop search emits `stopDiscoveryRequested`. Cancel stops discovery only with no active share; otherwise it cancels the share. Both are blocked while an action process is busy.                  |
| `PeerChoiceView.qml:180-214`                                                   | Separate Stop searching/Search again and Cancel controls exist. Both are disabled by the same global `actionBusy`.                                                                               |
| `StatusProbe.qml:174-181`, `runAction`                                         | One CLI action runs at a time. Competing requests return false, without a queue.                                                                                                                 |
| `StatusProbe.qml:105-125`, `acceptSnapshot`                                    | A validated JSON snapshot replaces `endpointSnapshot`; it is not merged as a patch. Missing discovery therefore does not retain the preceding searching value.                                   |
| `StatusProbe.qml:211-267`                                                      | Action exit clears busy, refreshes protocol/health/status, then one-second snapshot polling resumes. There is no event subscription. Polling is not tied to popup visibility.                    |

The two pages express the original receive-versus-recipient-choice workflow. They are not required by the control protocol. `EndpointSnapshot` already separates `active_share`, `discovery`, `peers`, and `visibility` in `crates/core/sharing/src/snapshot.rs:117-129`. Keep those independent facts; remove the exclusive page switch between receive controls and sending preparation.

Consent, waiting, transfer, and terminal phases are required state, but do not require separate application pages. Existing `ConsentView.qml`, `TransferView.qml`, and `TerminalView.qml` can remain specialized sections inside one panel. `PeerChoiceView.qml` can remain a recipient-list component after its separate hero/page assumptions are removed. A new navigation framework is unnecessary.

## Stop/search/capture findings

### Proven defects and couplings

1. `StatusProbe.discover` does not return `runAction`'s Boolean. Consequently `BarWidget.open` always takes its `!status.discover()` fallback and calls `refresh`, even when start was dispatched successfully. This is a real return-contract defect, not proof of the reported stop failure.
2. Stopping discovery does not mean cancelling `waiting_for_peer`. `SharePanel.viewState` intentionally retains the picker for that share. Retained peers also select the picker even when discovery is idle. Returning to a receive-only page is therefore not a reliable indication that discovery stopped.
3. Pre-device paste is stored, but selection without a daemon share rereads the clipboard. If the clipboard changes after capture, the preview and sent value can differ. If it becomes unavailable, sending fails despite a valid captured value. This defeats an independent draft.
4. A captured value has no editable control. When stop returns the snapshot to idle with no peers or active share, the idle branch does not display the captured draft. Adding a button to escape the picker alone would not fix preparation.
5. During an active share, `previewAttachment` and `previewText` prefer the daemon attachment, even if a new local paste was captured. The draft and in-flight attachment are conflated visually.

### What the daemon actually does

`crates/app/src/cli/dispatch.rs:127-130` sends a finite discover action through local control. `crates/app/src/daemon.rs:108-124`, `endpoint_response`, stops the sharing snapshot and queues network stop before returning Applied. `EndpointSnapshot.stop_discovery`, `snapshot.rs:284-287`, sets discovery idle without clearing peers or cancelling the active share.

The production worker does more: `crates/app/src/daemon/network/worker.rs:245-253`, `handle_command`, disables discovery, stops the browser, closes discovery leases, and emits peer-loss events. Therefore a retained-peer picker can be transient or arise without the production worker. It is not evidence that the production worker permanently keeps all peers after stop.

The daemon bounds searches in `crates/app/src/daemon/lifecycle.rs:119-143`, `timeout_discovery`. The snapshot has searching, idle, and timed-out states, but no stop-pending, cleanup-failed, or action identity field. An Applied response means the control action was accepted, not that radio cleanup has completed.

### Unresolved hypotheses, not diagnoses

- A CLI action that remains running keeps `actionBusy` true and disables Stop. Prediction: the UI never emits another stop action and the action process has not exited. Use a delayed fake executable to observe this without networking.
- Snapshot refresh/order or a failed availability probe may hide or lag updated state. Prediction: a successful fake stop followed by an idle snapshot changes the caption and enabled controls; failing or delayed status responses distinguish recovery from stop failure.
- The user may encounter a waiting share or retained peers rather than a running search. Prediction: an authoritative idle snapshot still renders the picker, but its caption becomes Search stopped and its button becomes Search again.
- The loaded shell instance or native binary may differ from current disk sources. Main compared SHA-256 checksums of installed `~/.config/omarchy/plugins/io.github.ollieedgeley.omarchy-quickshare/{BarWidget,SharePanel,StatusProbe}.qml` with their repository copies on 2026-09-06; all three matched exactly. This eliminates disk-version mismatch for these QML files, but does not prove which code the running shell loaded or how focus and shortcuts behave.

There is no source evidence that omitted `discovery` retains searching: `acceptSnapshot` replaces the snapshot, and `discoveryMessage` uses strict string comparisons.

## Smallest isolated probe and missing seam

Existing `tools/release/tests/panel-harness.qml`, `verifyDiscovery` and `verifyClipboardBrowsing`, exercise real `SharePanel` state bindings and emitted signals. They do not deliver an authoritative post-stop snapshot through `StatusProbe`, nor send a captured draft through the composed `BarWidget`. The status harness covers availability and basic busy paste but not this round trip. The source-regex assertions in `plugin-release-contract.test.mjs:196-224` cannot prove user-visible stop or draft behavior and should be replaced, not extended with more regex contracts.

The next implementation needs one composed UI/local-control probe using the real plugin with fake external CLI and `wl-paste` programs on an isolated PATH. Reuse the existing QML setup and stubs in `tools/release/tests/plugin-harness-stubs.mjs` and `plugin-release-contract.test.mjs:108-159`. No new permanent application abstraction is required solely for testing.

The fake CLI must keep state across start, stop, and status calls, and independently delay action and snapshot completion. The fake clipboard returns A for capture and B on a later read. Drive open, capture, stop, authoritative idle, select/send. Assert the displayed draft remains A, is editable, and the outbound argument is the final edited draft, never B. This fails the current captured-preview contract. A separate retained-peer/waiting-share case distinguishes stopping discovery from returning to another page. A Qt offscreen run with fake external processes is programmatic evidence, not proof of native keyboard focus.

Main executed the current `SharePanel.viewState` binding in JavaScript Eval with synthetic state on 2026-09-06. Observed results were searching with no draft → `peer_choice`; stopped with a retained peer → `peer_choice`; stopped with no peers → `idle`; stopped with a queued `waiting_for_peer` share → `peer_choice`. This proves the view-selection behavior, not a stuck radio or a live keyboard failure. Main also confirmed the late clipboard-read handler in source. No native visual inspection was available.

## Recommended unified state model

These are proposals for grilling, not accepted implementation decisions.

Keep one panel header with top-right Settings, one visibility indicator, one editable outgoing draft, a recipient list/search status, and the current share section. Settings is a subview or overlay, not a second send/receive destination. Visibility must remain readable while sending; opening the panel must never implicitly open inbound visibility.

Use the smallest independent state set:

- The existing daemon snapshot remains authoritative for discovery, visible peers, visibility, consent, transfer, and terminal outcome.
- `BarWidget.qml` owns transient `draftContent` and `selectedPeerId`; opening, stopping, polling, and receiving do not overwrite them. Drafts are not durable preferences and should not be written to config by default.
- A send action freezes the exact draft and recipient into the daemon request. The in-flight attachment stays separate from any later draft. No clipboard reread occurs at selection or Send unless the user explicitly requests replacement from clipboard.
- A pending command is separate from background searching. Draft editing remains enabled during start/stop and availability recovery. Dangerous or conflicting transfer actions remain serialized.

| Transition                          | Recommendation                                                                                                                                                                                                                     |
| ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Closed to open                      | Start one bounded outbound search when ready. If availability is still checking, remember one open intent and start after readiness. Repeated `open` while already open does not restart the timer. Never open inbound visibility. |
| Stop                                | Stop search only, preserve draft and recipient intent, show stopping until an authoritative result or explicit error. Do not cancel an accepted/queued share. Do not retry automatically.                                          |
| Close or Escape                     | Hide without cancelling a share or erasing the draft. Stop a panel-owned browse; do not blindly stop a CLI-owned search. Ownership is unresolved below.                                                                            |
| Reopen                              | Show existing share and draft immediately. Start a fresh bounded browse when no share forbids it. Do not reset transfer progress or auto-send.                                                                                     |
| Timeout                             | Show a distinct completed/empty search state and Search again. Draft remains editable. No perpetual scan loop.                                                                                                                     |
| Search again                        | Begin a fresh bounded search after confirmed completion/error. Duplicate start requests must not silently extend discovery forever.                                                                                                |
| Select peer                         | Store stable peer ID, never list index or display name. Selecting alone should not reread or send content; use explicit Send to make an editable draft safe.                                                                       |
| Peer disappears                     | Preserve draft. Mark the selection unavailable and disable Send; never choose a replacement automatically. Rediscovery by the same current peer ID can restore availability, but is not identity trust.                            |
| Incoming consent/transfer           | Keep the existing draft intact. Show incoming consent/transfer prominently in the same panel. Do not allow starting a conflicting share; do allow draft editing.                                                                   |
| Completion, reject, cancel, failure | Show the terminal outcome without destroying an unrelated next draft. A retry is an explicit send attempt, not a restart caused by reopening.                                                                                      |

## Decisions and implementation owners

- `BarWidget.qml` owns open/close intent, draft capture/edit lifecycle, recipient selection, and dispatch. Remove the discarded discover-return contract, redundant refresh behavior, and captured-value reread during implementation.
- `SharePanel.qml` owns layout and keyboard targets. Remove idle-versus-picker exclusivity, add independent sending/receiving/settings controls, retain consent and transfer safety gates, and preserve focus when snapshots update.
- `PeerChoiceView.qml` owns rows and search controls. Keep search state distinct from transfer cancellation; rename ambiguous Cancel if retained. `AttachmentBadge.qml` remains a preview, not the editor.
- `StatusProbe.qml` owns readiness, command completion, authoritative snapshots, and recovery. Decide polling versus a real event subscription with Main's broader plan; current polling must not be described as subscribed. Return dispatch success consistently and preserve action-specific errors.
- `crates/app/src/daemon.rs`, `daemon/lifecycle.rs`, `daemon/network/worker.rs`, and `crates/core/sharing/src/snapshot.rs` own bounded discovery and stop cleanup. Avoid a broad state-machine rewrite. If close must stop only panel-owned discovery while CLI shares continue, the current global start/stop protocol lacks ownership; decide an explicit request token/lease or a documented shared-search rule before implementation.
- `tools/release/tests/panel-harness.qml`, `status-harness.qml`, and `plugin-release-contract.test.mjs` own the composed regression cases. The future gate should exercise behavior rather than pinning source text or captions.
- Update `docs/research/quick-share-discovery-lifecycle.md:83-85`, which says opening an empty panel does not scan. The user has superseded that policy. Also update `packaging/omarchy-plugin/README.md` and any changed plugin export allowlist only when implementation changes their contract. This research does not silently rewrite old policy as already implemented.

Grilling must settle close ownership, whether active transfers suppress on-open browsing, explicit Send versus selection-to-send, draft retention on successful submission, and whether Settings opening/closing counts as a discovery transition. Recommended defaults above minimize accidental sends and background scanning.

## Behavioral acceptance cases

1. Opening an empty ready panel starts exactly one bounded outbound search. Receive status, top-right Settings, and editable draft remain available. Incoming visibility is unchanged.
2. Opening while readiness is checking starts once after readiness; closing first cancels that intent. Repeated show/open calls do not prolong the scan.
3. Capture A before any peer exists, edit it to C, stop search, and let status omit its idle discovery field. C remains visible/editable; no page navigation is needed.
4. Change the external clipboard to B, discover/select a device, and Send. The daemon receives C, not B. An empty later clipboard does not erase C.
5. Stop works with zero peers, retained peers, and `waiting_for_peer`. The UI distinguishes a stopped search from a cancelled share. The worker eventually releases discovery resources; search failure is reported without clearing the draft.
6. While start/stop is delayed or fails, the user can still edit/capture. Retry remains explicit, and failure never strands the UI behind a permanent busy flag.
7. Timeout preserves the draft and offers Search again. Reopen follows the agreed bounded-search rule; neither timeout nor a status update starts a perpetual retry loop.
8. Peer reordering/pinning does not retarget selection. Peer disappearance disables Send rather than silently choosing another recipient.
9. Receiving, waiting for consent, transfer progress, cancellation, and terminal outcome remain in the unified panel without overwriting an independent outgoing draft. Closing never cancels a transfer implicitly.
10. Keyboard Paste, editing focus, Stop, recipient navigation, Send, and Settings work on the actual Omarchy shell. A headless signal harness cannot substitute for this final native UI verification.
