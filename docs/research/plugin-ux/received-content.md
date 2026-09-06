# Received content auto-open (point 6)

Research date: 2026-09-06. Read-only. Does not change associations or launch apps.

Main design direction to evaluate, not accepted: auto-open only after a validated user-authorized receive completes; received content never becomes executable shell input.

## Proven current behavior

Inbound files stage under `ReceiveTarget` then `StagedFile::commit` in `crates/platform/storage/src/staging.rs`: flush, `sync_all`, declared-size check, `hard_link` to destination, remove staging. Failures skip publish. `crates/app/src/daemon/network/inbound.rs` `receive_share` commits only after `receive_payload` succeeds; cancel and protocol error skip commit.

Text and URL payloads never persist as files. `receive_payload` returns `NetworkEvent::InboundCompleted { value: Some(...) }`. `Daemon::apply_network_event` in `crates/app/src/daemon/production.rs` replaces the in-memory attachment then `sharing.complete`. No disk write.

Completion side effect today is a body-less Freedesktop notification (`crates/app/src/daemon/notify.rs`). No open, reveal, or editor launch. Config (`crates/app/src/config.rs`) has `receive_directory` plus timeouts and `pinned_peer_id`. No auto-open keys. Plugin (`packaging/omarchy-plugin/StatusProbe.qml`) only runs `omarchy-quickshare` argv arrays. `packaging/systemd/omarchy-quickshare.service` is `WantedBy=default.target` with no graphical `After=`.

Consent stays local: offer announced, `wait_for_consent`, then transfer. Feasibility already forbids interpolating names or payloads into shell (`docs/quick-share-feasibility.md`).

## Omarchy defaults (v4.0.2, installed 4.0.2-1)

Pinned sources: [mimeapps.list](https://github.com/omacom/omarchy/blob/v4.0.2/default/applications/mimeapps.list), [omarchy-launch-browser](https://github.com/omacom/omarchy/blob/v4.0.2/bin/omarchy-launch-browser), [omarchy-launch-editor](https://github.com/omacom/omarchy/blob/v4.0.2/bin/omarchy-launch-editor), [omarchy-default-browser](https://github.com/omacom/omarchy/blob/v4.0.2/bin/omarchy-default-browser), [omarchy-default-editor](https://github.com/omacom/omarchy/blob/v4.0.2/bin/omarchy-default-editor). Installed copies match under `/usr/share/omarchy/`.

| Concern           | Mechanism                                                                                                                       | Default                                                             |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| Folders           | MIME `inode/directory=org.gnome.Nautilus.desktop`                                                                               | Nautilus                                                            |
| Web               | `xdg-settings get default-web-browser`, fallback `xdg-mime query default x-scheme-handler/https`; `BROWSER` unset when querying | Chromium in mimeapps; user may change via `omarchy-default-browser` |
| Text files (MIME) | `text/plain=nvim.desktop` and related text types                                                                                | nvim desktop entry                                                  |
| Omarchy editor    | `$HOME/.local/state/omarchy/defaults/editor`, else `nvim`; launch via `omarchy-launch-editor`                                   | Independent of MIME                                                 |

Do not write those files. Store Quick Share choices in `config.toml`.

There is no `omarchy-launch-file-manager`. Browser launch uses `uwsm-app` under `systemd-run --user`. Editor TUI path uses `omarchy-launch-tui`; GUI editors use `uwsm-app`.

## Open received location versus open file

[xdg-open](https://portland.freedesktop.org/doc/xdg-open.html) opens a path or URL in the user's preferred application. For a directory that is the MIME default for `inode/directory` (Omarchy: Nautilus). That is the user-facing "default file manager" without talking to a specific D-Bus name.

[FileManager1](https://www.freedesktop.org/wiki/Specifications/file-manager-interface/) (edited 2021-05-07) `ShowItems` highlights a file in its parent. `ShowFolders` opens the folder. Those methods go to whoever owns `org.freedesktop.FileManager1`, which may not be the MIME default. Portal [OpenURI.OpenDirectory](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.OpenURI.html) (interface v5) is ShowItems via fd, then fallback OpenURI on the parent.

User ask: open the received-file location in the default file manager. Recommendation: after commit, `xdg-open` the parent directory of the committed path (usually `receive_directory`). That uses MIME `inode/directory`. Do not `xdg-open` the file itself; that would open its type handler. Do not call FileManager1 as the default path. Optional later: ShowItems only after a check that the FileManager1 owner is the same desktop id as `xdg-mime query default inode/directory`.

## URL auto-open versus arbitrary URI

xdg-open documents file, ftp, http, https. Omarchy browser launcher expects a URL argument after resolving the https handler. Omawrite's own URL sanitizer (`Backend::normalizedLinkUrl` in [omawrite v0.5.0 backend.cpp](https://github.com/omacom-io/omawrite/blob/v0.5.0/src/backend.cpp)) allows only http, https, ftp, mailto.

Recommendation: auto-open URLs only when the received string parses as `http` or `https` with a host. Reject `file:`, `javascript:`, `smb:`, custom schemes, and multi-line paste. Pass the URL as one argv element to `omarchy-launch-browser`, never through a shell. Do not call `xdg-open` on the raw payload; it would follow other schemes.

## Text persistence versus selected editor

Inbound text lives only in the completed share snapshot. Auto-open in an editor needs a local file. Recommendation: after complete, write UTF-8 under `receive_directory` with a generated safe name (same `safe_file_name` rules as files), then launch. Do not pipe text on stdin. Do not treat the payload as a command.

Selector: store a desktop id in config, default empty meaning Omarchy editor (`omarchy-launch-editor` on the persisted path). Settings UI lists `omawrite.desktop` plus `Gio.AppInfo.get_all_for_type("text/plain")` / `text/markdown` names ([Gio.AppInfo.get_all_for_type](https://docs.gtk.org/gio/type_func.AppInfo.get_all_for_type.html)). Persist the id only. Launch with GIO `g_app_info_launch` passing a `GFile` for that path ([Gio.AppInfo.launch](https://docs.gtk.org/gio/method.AppInfo.launch.html)), or `omarchy-launch-editor` when the id is empty. Do not parse desktop `Exec` field codes. Do not use Quickshell `DesktopEntry.execute()`: it ignores field codes so the path would not be passed. Do not spawn `execString`.

## Omawrite (0.5.0)

Installed `/usr/share/applications/omawrite.desktop`: `Exec=omawrite %f`, MIME `text/markdown;text/x-markdown;text/plain`, `StartupWMClass=omawrite`. Package URL [omacom-io/omawrite](https://github.com/omacom-io/omawrite). [src/main.cpp v0.5.0](https://github.com/omacom-io/omawrite/blob/v0.5.0/src/main.cpp): if `args.size() > 1 && !backend.modified()`, `backend.open(QUrl::fromLocalFile(args.at(1)))`. [Backend::open](https://github.com/omacom-io/omawrite/blob/v0.5.0/src/backend.cpp) returns "Only local files can be opened." for non-local URLs.

Source confirms path handling: argv[1] is treated as a local file URL. This binary was not launched in this research. Uncertain without running it: extra args, `-` stdin, directories, missing files (open reports could-not-open). Not a URL handler.

## Proposed config (evaluate)

All in existing `Config` / `config.toml`. Defaults off.

```toml
open_received_folder = false
open_received_url = false
open_received_text = false
received_text_app = ""   # empty: omarchy-launch-editor; else desktop id, e.g. omawrite
```

Three booleans match the user request. `received_text_app` is the honest extra key for "default or chosen app such as Omawrite" without rewriting MIME.

## Who launches

Daemon owns eligibility: `InboundCompleted` after commit or in-memory complete, consent already accepted, not cancelled/failed/rejected. A completed share may still be visible in status after plugin restart. An in-memory "seen id" set in QML does not survive that. Need a daemon-owned per-share claimed action (persist with the share, or a one-shot claim RPC) so at most one open attempt happens.

Desktop spawn still needs a graphical session. Daemon unit is `WantedBy=default.target` with no `After=` on the compositor. Locked session, missing plugin, or headless: skip the spawn and record the claim as not done or deferred with an explicit policy. Recommendation to grill: if the plugin is absent, do not launch from the daemon; leave the claim unclaimed until a session client takes it, or expire without opening. If the session is locked, wait until unlock or skip. Do not retry forever.

If a session client launches, use GIO or the verified Omarchy launchers (`omarchy-launch-browser`, `omarchy-launch-editor`) with argv, never a shell. Do not auto-open from notification actions. Current notify has empty actions.

## Owners

| Piece                                       | Owner                                                                |
| ------------------------------------------- | -------------------------------------------------------------------- |
| Config keys + parse/set                     | `crates/app` `config.rs`                                             |
| Terminal inbound event with committed paths | `quickshare-sharing` snapshot + `daemon/network` `InboundCompleted`  |
| Atomic file publish                         | `quickshare-storage` `StagedFile::commit` (already)                  |
| Text persist for open                       | `quickshare-storage` under receive dir, only if `open_received_text` |
| URL scheme allowlist                        | `crates/app` before launch                                           |
| Spawn                                       | session plugin via GIO or Omarchy launchers; daemon owns claim       |
| Selector UI                                 | `packaging/omarchy-plugin` settings; writes desktop id only          |
| Tests                                       | control/system: no launch on fail/cancel; no shell; http-only URLs   |

## Acceptance (failures included)

1. File receive, `open_received_folder=true`, commit ok: default folder app opens the parent directory (xdg-open on that directory). The received file is not opened.
2. Same with flag false: notification only.
3. Cancel or size mismatch: no destination, no xdg-open.
4. Duplicate name (`Error::Collision`): no open of the older file.
5. URL `https://example.com`, flag true: `omarchy-launch-browser` with that one argument.
6. URL `file:///etc/passwd` or `javascript:...`: no launch.
7. Text, flag true, empty app: persist then `omarchy-launch-editor` path.
8. Text, `received_text_app = "omawrite"`: GIO launch of `omawrite.desktop` with one local `GFile`.
9. Plugin restart after complete: no second launch; claim is daemon-owned.
10. Failed receive: no launch even if flags true.
11. Locked or no plugin: no desktop spawn; claim policy as in Who launches.

## Unresolved

Exact claim storage and locked/missing-plugin policy. Whether text auto-open should persist a `.txt` when the user never opted into keeping files. Whether ftp/mailto URLs should ever open. Grill later.
