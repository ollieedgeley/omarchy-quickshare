# Visibility, durable policy, and inbound consent

Research date: 2026-09-06. Audit only. No product decision is accepted by this note.

Main design to evaluate, not treat as shipped: durable on/off in config; daemon-owned expiring override in runtime; outbound discovery independent of inbound visibility; opening the panel is not consent.

## Proven current behavior

### Config file

Path is `$XDG_CONFIG_HOME/omarchy-quickshare/config.toml`, else `$HOME/.config/omarchy-quickshare/config.toml` (`config_path` in `crates/app/src/config.rs`). Missing file loads `Config::default()`. Unknown keys fail parse. `Config::save` writes the whole file after `create_dir_all`. `Config::set` mutates one key then `save`.

Keys today: `device_name`, `pinned_peer_id`, `receive_directory`, `discovery_timeout_secs` (15), `visibility_timeout_secs` (300), `transfer_timeout_secs` (120). Packaging default: `packaging/systemd/omarchy-quickshare.toml`. Local install copies that file only if the target is absent (`installConfig` in `tools/release/local-install.mjs`).

There is no durable discoverability boolean. Visibility is not a third persisted enum either. The file only stores the window length.

CLI `config set` / `config show` (`ConfigCommand` in `crates/app/src/cli/args.rs`, `config_command` in `crates/app/src/cli/dispatch.rs`) read and write the file. They do not talk to the daemon.

### Daemon load and non-reload

`lifecycle::run` / `run_simulated` call `Config::load` once, then `install_config`. That copies `pinned_peer_id` into the sharing coordinator and stores the rest on `Daemon`. There is no inotify, SIGHUP, or control request that reloads the file.

Live pin/unpin does write the file (`persist_pin`, `unpin_peers`). Timeouts and receive directory used after start stay at the process copy. `NetworkWorker::start` takes `consent_deadline` from `config.visibility_timeout_secs` at start. Consent wait and inbound radio window therefore share one number.

External edits while the daemon runs: next `config show` sees the file; the running endpoint does not. That is split-brain until restart. Failed `save` on pin returns an IO error to the control client; memory pin may already have applied (`share_response` pins then `persist_pin`).

### Visibility runtime

Public snapshot is `VisibilityState::{Closed, Open}` (`crates/core/sharing/src/snapshot.rs`). Default Closed. Control: `Request::OpenVisibility` / `CloseVisibility` (`crates/core/control/src/request.rs`). CLI: `visibility open|close`. Plugin idle row: `SharePanel.toggleVisibility` -> `StatusProbe.setVisibility` -> same CLI.

`Daemon::endpoint_response` flips sharing state then tells `NetworkWorker`. Worker `OpenVisibility` closes prior leases and calls `open_visibility` (BLE/Classic plus LAN listener). `CloseVisibility` drops leases. During inbound consent, `wait_for_consent` treats `CloseVisibility` as cancel (`crates/app/src/daemon/network/inbound/consent.rs`).

Lease clock: `Daemon.visibility_opened_at: Option<Instant>`. `timeout_visibility` in `lifecycle.rs` runs from `serve_until` every ~5ms idle. If snapshot is not Open, the Instant is cleared. If Open, `get_or_insert_with(Instant::now)` then compares against `config.visibility_timeout_secs`. OpenVisibility does not set the Instant itself. First timeout tick after open starts the clock. Re-open while already Open does not reset the Instant. The clock is `std::time::Instant`. On Linux that is CLOCK_MONOTONIC and does not include time spent suspended, so a 10-minute product window can last much longer across lid-close. Restart: snapshot and Instant are process memory. New daemon starts Closed even if the previous window had time left. No persistence of remaining lease.

Default window is 300s, not Google's 10 minutes. Google Everyone visibility returns after 10 minutes ([Use Quick Share](https://support.google.com/android/answer/15728591?hl=en), cited in `docs/research/quick-share-discovery-lifecycle.md`).

Repeated close is idempotent Closed. Disable during transfer: close cancels an in-consent inbound; already transferring uses transfer timeout separately.

### Plugin restart and snapshots

Plugin is a bar widget (`packaging/omarchy-plugin/manifest.json`). `StatusProbe` health-checks then `status --json` once, then every 1s while `protocolState === "ready"`. Catchup is poll of the snapshot, not a push event bus. Shell reload of plugin QML restarts probes from checking.

`BarWidget.open` sets `popupOpen` and calls `status.discover()` unless that action is busy, in which case it `refresh()`es. It does not open visibility. Consent-triggered UI must not call this `open()` as-is, because that starts or restarts outbound search while an inbound offer is waiting. `IpcHandler` target `io.github.ollieedgeley.omarchy-quickshare`: `open`/`show`/`toggle`/`close`/`hide`/`paste`. Host IPC: `omarchy shell io.github.ollieedgeley.omarchy-quickshare open` (`packaging/omarchy-plugin/README.md`). Omarchy panel host is `KeyboardPanel` in `/usr/share/omarchy/shell/Ui/KeyboardPanel.qml` (layer-shell, `open` bool). Plugin manual: [32-shell-plugins.md at b71dcad](https://github.com/omacom/omarchy/blob/b71dcad96e9d0b2962b7d225828a5cb6000ad720/manual/32-shell-plugins.md).

### Incoming consent vs panel open

Inbound offer: worker `announce_offer` -> `NetworkEvent::InboundOffered` -> `offer_inbound_sized` + PIN (`production.rs`). Phase `awaiting_local_consent`. Accept is `Request::Accept` then `network.accept_inbound`. Timeout of consent uses the same visibility_timeout duration. Notifications (`notify.rs`) fire only for sent/received/error completions, not for waiting consent.

`SharePanel` shows consent when the snapshot phase is `awaiting_local_consent`, but only if the panel is already open. `BarWidget` never sets `popupOpen` from that phase. Auto-open today is only paste-submit catchup (`onActionFinished` / `onEndpointSnapshotChanged` when paste pending). Opening UI is not auto-accept.

Locked session: lock IPC is `lock` in `/usr/share/omarchy/shell/plugins/lock/Service.qml`. Quick Share does not query it. Daemon can still advertise and wait for accept while locked. Plugin panel can still be toggled if the shell is running; the lock surface covers it. Missing plugin or missing binary: daemon visibility still works via CLI; there is no desktop consent UI. Plugin reports missing/incompatible/unavailable and does not install a binary.

## Confirmed gaps

1. No durable On/Off preference. Every receive path is a timed Open with default 5 minutes.
2. Config file is not live-reloaded. `config set` and hand edits diverge from the running daemon except pin writes.
3. Visibility timeout and inbound consent deadline are one config key.
4. Plugin does not auto-open on inbound consent. User must already have the panel, or use CLI accept.
5. No remaining-lease field in the snapshot. UI only has `visibility === "open"`.
6. Restart drops Open. Suspend clock is Instant, not wall.
7. Consent wait is not a desktop notification.
8. Settings gear / file-backed prefs UI does not exist in the plugin.

## Smallest interfaces (proposed, not accepted)

Keep three facts, not a new service.

1. Config key `discoverable` bool (name TBD). File remains source of truth for durable policy. Owner: `Config` + `config set`. Daemon must apply after write without a second store.
2. Runtime lease: keep `VisibilityState` plus `visibility_opened_at` in the daemon only. Snapshot may add `visibility_remaining_secs` so the plugin can show a countdown. Do not persist remaining time.
3. Control: keep `OpenVisibility` / `CloseVisibility`. Semantics:
   - Durable On: Open is the steady state; timeout does not close (or timeout is unused). Decision needed.
   - Durable Off: Open starts a 10-minute (or configured) lease; expiry returns Closed; Close cancels the lease immediately. Re-open while Open: decision whether to refresh the Instant.
4. Plugin: on snapshot `phase === awaiting_local_consent` and session not locked, map the panel by setting `popupOpen` (or a sibling that does not call `discover()`). Do not call `BarWidget.open()`. Do not call `accept`. Poll already present.
5. Failed config write: do not claim applied. Daemon-owned validated atomic save then apply. Same rule for pin. `config set` and plugin Settings must go through that path so memory and file cannot diverge.

Do not stat the config file on the existing ~5ms `serve_until` idle loop (about 200 Hz). That fights the project's resource goal. External hand edits need an explicit reload control request, or later a measured bounded/event-driven detector (inotify, or a slow timer far below 1 Hz). Leave which of those two for grill. No new watcher crate unless grill picks event-driven.

## State transitions to grill

Durable Off, lease idle: Closed, not advertised.

User "visible for 10 minutes": OpenVisibility, wall-elapsed start recorded now, radios on. Indicator Open + remaining. Expiry must use suspend-safe elapsed time, not Instant-only. Recommendation: compare against CLOCK_BOOTTIME or `SystemTime` start plus 10 minutes of wall time, documented so lid-close still ends the public window.

Lease expiry: close radios, Closed. Durable still Off.

User Close during lease: Closed immediately. Instant cleared.

Durable On: advertised until Off. Restart: On means Open after `install_config` (today restart is always Closed).

User Off while On: Close radios, persist Off.

Inbound while advertised Closed is a race, not an impossibility. A sender can connect and `InboundOffered` can land after CloseVisibility, or Close can run during `wait_for_consent`. Do not treat radio-down as proof there is no inbound session. Requirement: Close cancels pending (not yet accepted) consent, as `wait_for_consent` already does on `CloseVisibility`. An accepted transfer that has entered `Transferring` keeps going under `transfer_timeout_secs`, not the visibility lease. Visibility expiry must not abort an accepted transfer. Consent TTL is a separate deadline from the visibility lease. Today both are `visibility_timeout_secs` at worker start; split them.

Inbound offer while Open, panel closed, session unlocked: map the panel without starting discovery, show consent, user Accept/Reject. Auto-open is not Accept.

Plugin restart mid-consent: poll snapshot, map the panel again if still `awaiting_local_consent`, still without `discover()`.

Plugin missing: daemon waits until consent deadline; then fail. No silent accept.

Locked: do not map the panel over the lock. Daemon keeps waiting. After unlock, next poll opens the panel if still awaiting.

## Ownership

| Concern                                     | Owner                                              |
| ------------------------------------------- | -------------------------------------------------- |
| File schema, path, save, parse              | `crates/app/src/config.rs`                         |
| Apply timeouts, install_config, pin persist | `crates/app/src/daemon/lifecycle.rs`               |
| Open/Close requests                         | `Daemon::endpoint_response`                        |
| Radio leases                                | `NetworkWorker` + `open_visibility`                |
| Consent wait / close-cancels                | `inbound/consent.rs`                               |
| Snapshot visibility + phase                 | `quickshare-sharing` snapshot                      |
| Control messages                            | `quickshare-control`                               |
| Poll + CLI actions                          | `StatusProbe.qml`                                  |
| Panel map                                   | `BarWidget.qml` `popupOpen` + host `KeyboardPanel` |
| Durable prefs UI                            | plugin Settings, not yet present                   |

## User decisions (unresolved)

- Persist On across daemon restart? Recommendation: yes for durable On; never persist a leftover Off-mode lease.
- 10 minutes vs keep `visibility_timeout_secs` (300). Recommendation: default 600 to match Google Everyone, keep the key. Expiry is 10 minutes of wall/boot time including suspend, not Instant-only.
- Refresh lease on repeated Open while Open? Recommendation: reset the start so "make me visible" is always a full window.
- Split consent deadline from visibility timeout? Recommendation: yes. Pending consent has its own TTL. Visibility expiry closes advertising and pending consent only. Accepted transfers use `transfer_timeout_secs`.
- External-edit consistency? Recommendation: daemon-owned atomic save/apply for all in-product writes. For hand edits, explicit `config reload` or a later measured event-driven/slow detector. Not 5ms mtime polling. Failed parse keeps last-good runtime and surfaces an error on `status`.
- Auto-open when locked? Recommendation: no. Wait until unlock.
- Consent-triggered panel vs `BarWidget.open()`? Recommendation: set `popupOpen` only; never start outbound discovery while `awaiting_local_consent`.

## Acceptance scenarios (for later implementation)

1. Durable Off, no lease: nearby senders do not see the endpoint. Snapshot Closed.
2. Durable Off, user starts 10-minute visibility: Open, indicator, radios up; after 10 minutes of suspend-inclusive elapsed time, Closed without writing On into the file. Lid-close does not extend the public window.
3. Close during lease: Closed immediately; file still Off. Pending consent cancelled. Accepted transfer continues until its own deadline or cancel.
4. Durable On, daemon restart: comes back Open without a click.
5. `config set` then running daemon matches without split-brain; failed write leaves previous file and previous runtime. Atomic save then apply, daemon-owned.
6. Hand-edit valid toml: converges only after explicit reload or the grilled detector; invalid toml: last-good runtime, error visible. No 200 Hz stat loop.
7. Inbound PIN while panel closed and unlocked: panel maps to consent without starting search; Accept still required.
8. Same inbound with plugin down: no auto-accept; offer times out or CLI accept.
9. Session locked: no panel; after unlock, consent still pending opens the panel without discovery.
10. UI open is never Accept.
11. Close/offer race: inbound that wins the race into consent is cancelled by Close; inbound that already accepted is not killed by visibility expiry.

## Sources

- `crates/app/src/config.rs`
- `crates/app/src/daemon.rs`
- `crates/app/src/daemon/lifecycle.rs`
- `crates/app/src/daemon/production.rs`
- `crates/app/src/daemon/network.rs`
- `crates/app/src/daemon/network/inbound.rs`
- `crates/app/src/daemon/network/inbound/consent.rs`
- `crates/app/src/daemon/notify.rs`
- `crates/core/sharing/src/snapshot.rs`
- `crates/core/control/src/request.rs`
- `packaging/omarchy-plugin/BarWidget.qml`
- `packaging/omarchy-plugin/StatusProbe.qml`
- `packaging/omarchy-plugin/SharePanel.qml`
- `packaging/omarchy-plugin/manifest.json`
- `packaging/omarchy-plugin/README.md`
- `/usr/share/omarchy/shell/Ui/KeyboardPanel.qml`
- `/usr/share/omarchy/shell/services/PluginRegistry.qml`
- `/usr/share/omarchy/shell/plugins/lock/Service.qml`
- [Omarchy plugin manual at b71dcad](https://github.com/omacom/omarchy/blob/b71dcad96e9d0b2962b7d225828a5cb6000ad720/manual/32-shell-plugins.md)
- [Use Quick Share (Everyone 10 minutes)](https://support.google.com/android/answer/15728591?hl=en)
- `docs/research/quick-share-discovery-lifecycle.md`
