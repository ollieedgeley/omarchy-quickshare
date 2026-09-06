# Plugin UX research and proposed implementation plan

Research date: 2026-09-06. Baseline: `4cb010d` on `main`.

Status: historical research proposal, not approved implementation. Subsequent user decisions supersede the extra Send button, optional manual reload, fixed panel-search deadline, and explicit outbound discovery Stop control proposed below. The user parked device-trust work.

The approved four-feature UX breakdown and twelve local tickets are indexed at `.scratch/quickshare-ux-plan.md`, following `.scratch/workflow.md`. They require automatic save/apply, clipboard-on-select default Off, capture/selection-triggered sending, and panel-lifetime discovery. Closing the panel ends outbound browse intent; there is no separate Stop/Search again interaction. The ten-minute inbound discoverability control remains separate. Source findings below remain research evidence; ticket publication does not authorize application changes.

## What the user is asking for

1. One main panel for sending and receiving, with Settings at the top right. Every durable setting lives in a config file. Discoverability has On/Off settings, plus a clearly indicated ten-minute window available from the main panel when Off.
2. A valid incoming request requiring PIN comparison automatically presents the panel for Accept or Cancel. Presenting the panel must not accept the request.
3. Fix the reported inability to stop searching and prepare pasted content before selecting a device. Capturing content must not depend on escaping a device-picker page.
4. Opening the panel starts searching for devices immediately, without making the laptop discoverable for incoming shares.
5. Explain Google's Your devices and whether our pinned preference can safely provide similar convenience.
6. Add separate settings for opening a received file's location, a received URL in the default browser, and received text in the default editor or a chosen application such as Omawrite.

All six belong to the upcoming UX round. Researching trust is not approval to add Google accounts, a companion application, or bypass authentication.

## Research findings

| Topic                | Finding                                                                                                                                                                                                                   | Evidence                                                                                     |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Two pages            | The current idle section contains receive visibility; the picker section replaces it during discovery or recipient selection. The protocol does not require that separation.                                              | [Panel audit](panel-audit.md)                                                                |
| Search on open       | Already attempted, but the discovery wrapper drops its dispatch result, causing an unnecessary fallback refresh. Readiness and repeated-open behavior need an explicit contract.                                          | [Panel audit](panel-audit.md)                                                                |
| Stop and paste       | Search state, queued shares, and page selection are coupled. Captured clipboard preview is not reliably the send source: selecting a peer can reread the clipboard. The exact live stuck-search cause remains unresolved. | [Executed binding probe and limits](panel-audit.md#smallest-isolated-probe-and-missing-seam) |
| Config               | Existing TOML supports timeouts and preferences, but no persistent discoverability or auto-open switches. Running daemon config does not follow file edits.                                                               | [Visibility/config audit](visibility-consent.md)                                             |
| Temporary visibility | Current runtime window defaults to five minutes; there is no countdown in the public snapshot. Visibility and consent currently share a timeout setting.                                                                  | [Visibility/config audit](visibility-consent.md)                                             |
| Incoming consent     | The panel can display consent but does not automatically open for it. State arrives through polling, not an event subscription.                                                                                           | [Consent audit](visibility-consent.md)                                                       |
| Your devices         | Google's account-backed identity is different from a preferred discovery identifier. Our current pin contains no authenticated durable peer credential.                                                                   | [Trust research](trusted-devices.md)                                                         |
| Content opening      | No auto-open implementation exists. Omarchy's selected editor and the MIME default for text files are independent. Omawrite's source handles a local filename argument.                                                   | [Received-content research](received-content.md)                                             |

The installed `BarWidget.qml`, `SharePanel.qml`, and `StatusProbe.qml` match the repository byte-for-byte. That rules out different on-disk versions for those files, not a stale loaded shell instance.

## Proposed experience

One stable main layout, with specialized request/progress sections rather than separate send and receive destinations:

```text
Quick Share                  [Visible 09:42 / Stop] [Settings]

Content to send
[Paste or prepare content                                  ]
[Captured content preview                         Clear    ]

Devices                              Searching... [Stop]
[Preferred phone] [Another available device]
[Send to selected device]

Incoming request / current transfer, when present
[Sender, content summary, PIN comparison, Accept / Cancel]
```

This is an information-layout sketch, not a final visual design. Settings is a subview or overlay with a return action. It does not reintroduce separate Sending and Receiving pages.

Draft, recipient selection, outbound discovery, inbound visibility, and active share are independent facts. The daemon remains authoritative for connections and transfer state. An in-memory draft is not a durable setting and should not go into the config file.

### 1. Config-backed discoverability and settings

Use the existing `$XDG_CONFIG_HOME/omarchy-quickshare/config.toml`, falling back to `~/.config/omarchy-quickshare/config.toml`. Do not add a competing plugin preferences file.

Recommended model:

- Durable Off: no incoming advertisement unless a temporary window is active.
- Durable On: advertise whenever the daemon and required adapters are available. Restore this preference after restart.
- Off plus temporary window: daemon owns a ten-minute expiry; the stored preference remains Off. A daemon restart drops this temporary permission.
- Temporary visibility is runtime state, not a third persisted preference.

The main header should distinguish Off, On, temporary visibility with remaining time, and unavailable/starting/error. Use text or an icon plus colour, not colour alone. Off exposes a **Discoverable for 10 minutes** action next to Settings; an active window exposes a countdown and Stop. Persistent On does not show a misleading ten-minute action.

The ten minutes must include time asleep, so resuming cannot unexpectedly extend public visibility. Start the displayed window from successful activation, report partial adapter availability honestly, and let the daemon enforce expiry even if the plugin exits.

Disabling visibility and cancelling a share are separate operations. Expiry must not terminate an already accepted transfer. The treatment of a pending, already-admitted consent request needs agreement; it must have its own deadline rather than inheriting the visibility duration accidentally.

Config changes need a single validated save/apply path. A failed write must not leave the UI or daemon claiming a new preference was applied. Hand-edited configuration needs a documented reload contract. Recommend live reload if inexpensive and reliable; do not poll the filesystem every five milliseconds. Invalid edits should preserve the last valid configuration and surface an actionable error. Preserve user comments and unrelated settings when editing an individual preference where the existing parser allows it; decide the file-editing contract before choosing a writer.

### 2. Automatic incoming consent presentation

React to a validated inbound share entering `awaiting_local_consent`, not merely a peer sighting or a connection attempt. Show sender, attachment summary, session PIN, and explicit Accept/Cancel controls in the unified panel.

- Opening is never acceptance or PIN verification.
- Present each request once; polling must not repeatedly steal focus after dismissal.
- Recover a still-pending request after plugin restart.
- Do not expose the request over the lock screen. After unlock, present it only if still pending.
- Preserve a prepared outgoing draft while presenting an incoming request.
- Use a presentation path that does not start a competing outbound search. The current general `open()` helper starts discovery and cannot be reused blindly.
- With no plugin, remain consent-gated. Do not infer approval from missing UI.

A deliberate panel opening by the user should start discovery when the endpoint is available and no active connection forbids it. An incoming-consent presentation is a different reason to open. Confirm this exception to the general on-open-search rule during alignment.

### 3. Stop-search repair and genuine draft capture

Fix the actual stop round trip before relying on the redesign to conceal it. There are confirmed composition defects, but no proven live-radio root cause yet.

Implement a draft whose displayed value is the value that will be sent. Paste captures once; editing changes that draft. Later clipboard changes, discovery results, Stop, timeout, and polling must not replace it. File references retain their existing validation at send time.

Stop search, Clear draft, Cancel transfer, and Close panel must have separate meanings. A slow search command should not disable text editing. Busy commands need bounded completion and an error state rather than a permanently disabled Stop control.

Recommend selecting a device followed by explicit Send. Selection alone should not silently read the clipboard or send a different value. This is a proposed interaction change that needs agreement, not a settled requirement.

Keep the existing single-active-share limitation. A local draft need not become a queued daemon share until sending begins. Do not add a multi-transfer queue just to combine the layout.

### 4. Discovery when the panel opens

Start one bounded search on a closed-to-open transition when ready. If readiness is still being checked, remember that intent and start once ready; cancel the intent if the panel closes first.

- Repeated `show` or state updates do not restart discovery indefinitely.
- Search expiry shows Search again; the draft remains usable.
- Opening Settings and returning is not a fresh search request.
- Peer reordering never changes the chosen recipient; losing the selected peer disables Send rather than picking a replacement.
- Closing never cancels an active transfer or clears the draft implicitly.

Search ownership on close is unresolved: the current start/stop interface is global. Prefer stopping an idle panel-started browse without disrupting a CLI-started share. First decide whether an explicit ownership token is warranted or a documented shared-search rule is sufficient. Do not introduce lease infrastructure without that requirement.

The existing [discovery lifecycle policy](../quick-share-discovery-lifecycle.md#recommended-daemon-policy) says opening an empty panel does not scan. The user's new requirement changes that policy. Update the old document with implementation, while retaining bounded scans and the separation from inbound visibility.

### 5. Preferred devices versus trusted devices

Google's [Android Help](https://support.google.com/android/answer/9286773?hl=en) and [Windows Help](https://support.google.com/android/answer/13801258?hl=en) explicitly tie Your devices and automatic receipt to the same Google Account. Pinned Google source shows certificate-backed session authentication and a separate self-share consent decision. See [the primary-source analysis](trusted-devices.md).

Our pin currently saves a discovery identifier. It is not a verified key, an account relationship, or a remembered PIN. A session PIN is compared between devices, not reused as a device password. Device names, endpoint IDs, and MAC addresses must not grant trust.

There is an additional current-code concern: a pinned sighting can automatically initiate queued outbound content, and the inspected send path has no explicit local PIN-confirmed barrier. A malicious recipient's Accept response does not authenticate that recipient. The spoofed-preference risk is a source-derived inference, not a demonstrated exploit. Resolve this interaction before advertising pinned sending as secure one-click trust.

Recommendation for this round: call it **Preferred device**, use it for ordering or selection, preserve authentication and explicit incoming consent, and reduce navigation rather than remove security checks. Agree on sender-side PIN verification before changing the automatic preferred-send behavior.

A durable, secure PINless relationship needs a credential bound during verified enrollment and proof of possession on future connections, plus revocation and key-rotation rules. No supported account-free enrollment path with unmodified Android was established. Google account support would be a separate researched product capability, not a small config option.

Official Quick Share QR sharing is another possible convenience route. It is a per-transfer scan, not persistent device trust. Its use with this endpoint remains a bounded future protocol investigation requiring stock-Android evidence; help-page UX alone does not prove Linux interoperability.

### 6. Post-receive opening settings

Three independent opt-in toggles, initially Off:

| Setting                     | Proposed action                                                                                                                                                                                                   |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Open received file location | Open the committed file's containing directory using the user's default file manager. Do not launch the received file itself. Selection/highlighting is optional only if the correct default handler supports it. |
| Open received URL           | Open a validated HTTP/HTTPS URL with the user's default browser. Never execute the value or pass arbitrary URI schemes to a generic opener.                                                                       |
| Open received text          | Save the received text as a local UTF-8 file through safe storage, then open it in the Omarchy default editor or the user's chosen text application.                                                              |

Add a text-application choice: **Omarchy default editor** or an installed application such as **Omawrite**. This selection belongs only to Quick Share; do not change global editor or MIME preferences. Omawrite's inspected desktop entry uses `omawrite %f`, and its source accepts a local path. It does not require becoming the default editor. Source support was inspected; no editor was launched during this audit.

Use existing Omarchy launchers or a native desktop-entry launcher with an argument list. Do not build a shell-command parser or a custom Desktop Entry Exec interpreter. Remote filenames and received text never become executable command strings.

Eligibility belongs to validated inbound completion, after consent and safe storage publication. An open failure is an action error after a successful receive, not a failed transfer. Preserve access to the received content and offer an explicit retry.

Do not trigger from arbitrary completed snapshots: polling, plugin reload, or daemon restart must not repeatedly launch content. Choose one completion/action owner and defined consumption semantics. Recommend desktop-side launching with daemon-authorized per-share eligibility; decide whether missing/locked desktop sessions skip the automatic action or retain a bounded pending action. Do not silently promise exactly-once external application launch across crashes.

Agree whether text auto-open leaves a normal `.txt` in the receive directory or uses a managed temporary document. Recommendation: a visible saved file, so closing the plugin cannot lose the text. Document retention and make its location apparent.

## Implementation slices after alignment

Each slice should be independently reviewable and green. This list is a plan, not work already performed.

1. **Stop/capture reproduction and repair.** Exercise the composed plugin through its CLI and clipboard seams. Fix stop/busy behavior, discovery dispatch, and draft capture with observable regressions. Keep the current layout until those contracts work.
2. **Config save/apply contract.** One file-backed settings path, atomic/validated updates, documented hand-edit reload, failure reporting, and explicit migration of existing timeout preferences. No duplicate plugin store.
3. **Discoverability policy and expiry.** Durable On/Off, ten-minute runtime override, snapshot countdown/status, restart/suspend behavior, and distinct consent lifetime. Reuse current radio owners.
4. **Unified main panel.** Replace idle/picker exclusivity with draft, devices, visibility, and active share sections. Add Settings entry and bounded on-open search behavior. Preserve keyboard focus and drafts.
5. **Incoming consent presentation.** Open once per request without starting discovery or accepting automatically. Handle lock, dismiss, cancellation, plugin restart, and unavailable UI.
6. **Post-receive actions.** Add each toggle as a small independent slice: folder location, HTTP/HTTPS browser, then stored text and app selection. Define completion consumption once; exercise failures and replay.
7. **Preferred-device interaction.** Apply agreed wording, selection/send behavior, and verification requirements. Do not add unproven account or trust functionality.
8. **Integrated native verification.** Exercise all interactions on the actual Omarchy shell, then real phone scenarios in both directions where behavior changed. Update plugin usage, export/control compatibility, and superseded policy documents together.

Ownership stays in the existing plugin QML, app config/composition, control messages, Sharing snapshot, and storage modules. Each research note names the relevant files. No new crate, navigation framework, hosted service, or application dependency is justified by this plan alone.

## Verification and evidence boundaries

Research performed:

- Repository/source audits and first-party Google, Omarchy, desktop, and Omawrite documentation reads.
- SHA-256 comparison of the three central installed QML files with repository copies: all matched.
- Executed the actual `SharePanel.viewState` JavaScript binding with synthetic snapshots: stopped-with-peer and stopped-with-queued-share remain in the picker; stopped-with-neither returns idle. This proves page-selection behavior, not a stuck physical scan.
- Native inspection attempted through the computer backend. Wayland capture is unavailable in this build; no visual UX or keyboard-focus claim is made.

Before implementation can be called complete:

- The composed UI probe must distinguish stop request, authoritative idle result, delayed/error responses, and retained peers. Capture A, change system clipboard to B, edit draft to C, select a device, and prove C is sent.
- Timer/config cases cover successful save/apply, failed write, invalid hand edit, restart, suspend, expiry, and accepted-transfer independence.
- Incoming requests open the panel once, preserve draft, remain consent-gated, and respect lock and cancellation.
- Open actions happen only for eligible inbound completions, never failed/rejected/cancelled receives, outbound sends, historical snapshots, or draft edits. Test arbitrary URI schemes and launch failures.
- Actual shell checks cover keyboard Paste/editing, Stop, selection, Send, Settings, countdown, automatic consent presentation, and default-app behavior. Offscreen signals do not replace native UI verification.
- Keep reference/phone evidence distinct. The successful morning inbound FILE test does not prove outbound authentication, clipboard UX, auto-open behavior, QR sharing, or Google account trust.

## Decisions for the later grilling session

1. Select then Send, or selecting a device sends the already-captured draft immediately? What should the existing preferred auto-send shortcut do?
2. Should draft text survive panel close, a successful send, or shell restart? Recommended: close yes, successful send explicit policy, restart no persistence by default.
3. Does ten-minute expiry preserve an already-admitted consent request under its separate deadline? Does explicitly turning visibility Off cancel that pending request?
4. How should close affect discovery started by the CLI? Should inbound/active-transfer presentations suppress the usual on-open search?
5. Should hand edits reload automatically, or through a documented reload command? What error should the panel show for an invalid file?
6. Should automatic openings be skipped while locked or queued until unlock? Should closing the consent panel dismiss presentation only or reject the request?
7. Is received text a saved `.txt` in Downloads, and is Omarchy default/Omawrite/installed-app selection sufficient?
8. Is preferred-device convenience with explicit authentication acceptable, or should account-backed/QR/controlled-peer enrollment receive a separate research phase?

These decisions are collected here rather than asked piecemeal during research. No implementation should infer a permissive answer to a security-sensitive choice.
