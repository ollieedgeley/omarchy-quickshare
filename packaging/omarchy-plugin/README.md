# Omarchy Quick Share plugin

This is the Omarchy shell frontend for the native `omarchy-quickshare`
binary. The bar icon opens a panel for outbound peer choice, inbound PIN
consent, receive visibility, progress, cancellation, and terminal errors.
Right-clicking a discovered peer pins or unpins it; only the native daemon
stores the single preferred peer and performs transfers.

The plugin reports missing, incompatible, and unavailable native runtimes
separately. It never downloads, installs, archives, or transfers content.

## Install

Install a published plugin repository with Omarchy:

```sh
omarchy plugin add <plugin-repository-url> --enable
```

For a local source checkout, build and install the native binary. Commit plugin
changes before exporting; the exporter records the exact commit and refuses
source drift.

```sh
cargo build --release --locked -p omarchy-quickshare
install -Dm755 target/release/omarchy-quickshare \
  "$HOME/.local/bin/omarchy-quickshare"
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

The plugin uses control protocol 5. Native status includes saved and applied
preferences, pending activation, and errors. Pin/unpin actions require the
daemon; `omarchy-quickshare config set pinned_peer_id pixel-8` also works offline.
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

## Shell actions and keybindings

Opening the panel starts discovery immediately. Nearby devices appear as they
are found. Clicking a device sends the content already captured in the badge
to that exact peer. Without captured content, the default Off setting waits
for an explicit Paste. Turning `read_clipboard_on_select` On lets that
recipient selection read the current clipboard once instead.

```sh
omarchy shell io.github.ollieedgeley.omarchy-quickshare open
```

The plugin also accepts Omarchy's universal-paste IPC action. If the panel is
open, this shows an attachment badge and sends to an explicitly selected
recipient if one is waiting. Otherwise clicking a device sends the captured
value without rereading the clipboard. If the panel is closed, the action
submits immediately to a visible pinned peer or opens the chooser while
discovery continues.

```sh
sh -c 'value=$(wl-paste --type text/uri-list --no-newline 2>/dev/null || wl-paste --no-newline); omarchy shell io.github.ollieedgeley.omarchy-quickshare paste "$value"'
```

For a file or folder, `wl-paste` supplies one `file://` URI. Plain text,
including line breaks, remains one quoted argument. The plugin forwards the
value as an argument array rather than evaluating peer names or attachment
text as shell input.

The native Arch package is not published yet. Building from the source checkout
is the current fallback. The plugin never downloads or installs the binary.
