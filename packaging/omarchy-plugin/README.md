# Omarchy Quick Share plugin

This is the Omarchy shell frontend for `omarchy-quickshare-daemon`.
The bar icon opens a panel for outbound peer choice, inbound PIN consent,
receive visibility, progress, cancellation, and terminal errors.
Right-clicking a discovered peer pins or unpins it; only the native daemon
stores the single preferred peer and performs transfers.

The plugin connects directly to the daemon's local socket. It reports
incompatible or unavailable service state and marks retained observations
stale after disconnection. It does not need the CLI on PATH and never
downloads, installs, archives, or transfers content itself.

## Install

Install a published plugin repository with Omarchy:

```sh
omarchy plugin add <plugin-repository-url> --enable
```

For a local source checkout, install the native CLI and daemon together.
Commit plugin changes before exporting; the exporter records the exact
commit and refuses source drift.

```sh
make install-local
make plugin-export
omarchy plugin add "file://$PWD/dist/omarchy-plugin" --enable --yes
```

For local UI testing without a phone, install the deterministic peer mode:

```sh
make install-local-simulation
```

Running `make install-local` again restores the normal service mode.

## Preferences

The native config is
`${XDG_CONFIG_HOME:-$HOME/.config}/omarchy-quickshare/config.toml`.
Valid CLI or editor changes apply automatically without restarting the daemon.
Use `omarchy-quickshare config show` to inspect saved preferences and
`omarchy-quickshare config set device_name "My laptop"` to edit them.
The CLI distinguishes applied, saved-offline, pending, and activation-error
outcomes. Accepted transfers keep their settings until they finish.

The exported `release.json` declares the required local control protocol.
The daemon and plugin must agree on that version. Native status includes
saved and applied preferences, pending activation, and errors. Pin/unpin
actions require the daemon. The CLI also updates an offline preference with
`omarchy-quickshare config set pinned_peer_id pixel-8`.
`discoverable` and `read_clipboard_on_select` default to `false`.
The daemon applies discoverability policy independently of acknowledgement
that all preferences have activated. The snapshot's `visibility_status` reports
the applied policy and runtime permission even while other preferences remain
pending. The plugin consumes the applied `read_clipboard_on_select` value,
not a pending saved change. Changing it alone does not read or send content.

## Receive visibility

The daemon owns receive permission, listener readiness, and expiry. The panel
distinguishes closed, starting, open, and unavailable. Only open means ready
to receive. Starting or unavailable can still have receive permission, so the
receive control remains able to request a stop in either state. Failed
activation reports unavailable with permission withdrawn and no countdown;
the control then requests a new attempt instead.

With `discoverable` off, `omarchy-quickshare visibility open` requests one fixed
ten-minute receive window. The countdown begins only after the current
activation installs its complete inbound resource bundle. Starting reports no
remaining time and consumes none of the window; partial activation does not
start it. Once running, elapsed time includes suspend.
Repeating open while starting or open does not extend the deadline.
With `discoverable` on, receiving has no temporary countdown.
No configurable visibility timer is exposed by the plugin.

Passive expiry stops new inbound offers but preserves an offer already waiting
for consent until its independent consent deadline. An explicit
`omarchy-quickshare visibility close`, or switching `discoverable` off, also
cancels pending inbound consent. Accepted transfers continue in either case.

## Terminal sharing

Pass one text value, URL, file, or folder directly to the native CLI. The
explicit `send` form is equivalent.

```sh
omarchy-quickshare "hello from Omarchy"
omarchy-quickshare ./photo.jpg
omarchy-quickshare "https://example.test/share"
```

Acknowledgement means the daemon accepted work, not that the recipient received
it. Keep the request ID printed by the CLI if acceptance is uncertain:

```sh
omarchy-quickshare operation <request-id> --json
omarchy-quickshare status --json
omarchy-quickshare protocol-version
```

The operation query reads retained metadata without retrying or resuming the
transfer. An unknown outcome does not prove nonacceptance. The panel reconciles
lost replies by operation identity and reconnects without resubmitting content.

The daemon stores its bounded journal at
`${XDG_STATE_HOME:-$HOME/.local/state}/omarchy-quickshare/operations.json`.
After daemon restart, previously active work is failed with reason `interrupted`;
the recipient may already have received content. Check the recipient before
deliberately sending again. There is no automatic payload replay or resume.
Saved preferences and committed received files remain separate from the journal.

The plugin requires local control protocol 7. This versions the daemon/CLI/panel
JSON API, not Google's Connections or Sharing wire protocol. Snapshot responses
have sibling `snapshot`, `preferences`, and `operations` fields; submission
envelopes require a `request_id`, generated by the client. Install compatible
local components. The deterministic peer mode above is not independent-peer
interoperability evidence.

## Shell actions and keybindings

Opening the panel starts discovery immediately. Nearby devices appear as they
are found. Clicking a device sends the content already captured in the badge
to that exact peer. Without captured content, the default Off setting waits
for an explicit Paste. Turning `read_clipboard_on_select` On lets that
recipient selection read the current clipboard once instead.

An explicit keyboard Paste takes priority over an automatic clipboard read
already in progress. The old result is discarded before the replacement
capture completes. If the replacement fails, the old automatic value is not
sent; the panel reports the failure and waits for another deliberate action.
The keyboard Paste shortcut belongs to the focused panel window, not the bar
window, so it also works when the popup holds keyboard focus.

Each clipboard capture has a three-second deadline across MIME fallbacks.
Timeout stops that read without automatic retries. Paste or select a device
again to retry deliberately. A newer queued explicit Paste keeps its intent
when an older automatic read expires.
Only file URIs and offered text MIME types are read. Unsupported non-text
clipboard data is not reinterpreted as a text share.

Panel submissions use typed socket requests. Text that looks like a filename
remains text even if that file exists in the working directory. File URIs
retain send-time daemon validation. Values such as `--help` are literal
content, not command-line options.

Closing or cancelling preparation clears captured content and recipient
intent. Clear content uses the same invalidation: late clipboard results and
queued replacement reads cannot restore the badge or start a share. Reopening
does not replay a send. To recover, paste again and choose a current device.
Closing the panel does not cancel an admitted transfer.

```sh
omarchy shell io.github.ollieedgeley.omarchy-quickshare open
```

The plugin also accepts Omarchy's universal-paste IPC action. If the panel is
open, this shows an attachment badge and sends to an explicitly selected
recipient if one is waiting. Otherwise clicking a device sends the captured
value without rereading the clipboard. If the panel is closed, Paste opens
it and captures the value for deliberate recipient selection. A preferred
device is never selected or sent to automatically by the panel.

```sh
sh -c 'value=$(wl-paste --type text/uri-list --no-newline 2>/dev/null || wl-paste --type text --no-newline; printf x); value="${value%x}"; omarchy shell io.github.ollieedgeley.omarchy-quickshare paste "$value"'
```

The temporary sentinel preserves trailing newlines during command substitution.

For a file or folder, `wl-paste` supplies one `file://` URI. The paste IPC
receives plain text, including line breaks, as one quoted argument. The
plugin encodes daemon requests as JSON rather than evaluating peer names or
attachment text as shell input.

The native Arch package is not published yet. Building from the source
checkout is the current fallback. The plugin never downloads or installs
the native executables.
