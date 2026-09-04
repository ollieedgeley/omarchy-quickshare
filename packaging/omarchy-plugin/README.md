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
are found. Clicking a device reads the current clipboard and submits its text,
URL, or single copied file/folder to that exact peer.

```sh
omarchy shell io.github.ollieedgeley.omarchy-quickshare open
```

The plugin also accepts Omarchy's universal-paste IPC action. If the panel is
open, this only shows an attachment badge; clicking a device reads the current
clipboard. If the panel is closed, the action submits immediately to a visible
pinned peer or opens the chooser while discovery continues.

```sh
sh -c 'value=$(wl-paste --type text/uri-list --no-newline 2>/dev/null || wl-paste --no-newline); omarchy shell io.github.ollieedgeley.omarchy-quickshare paste "$value"'
```

For a file or folder, `wl-paste` supplies one `file://` URI. Plain text,
including line breaks, remains one quoted argument. The plugin forwards the
value as an argument array rather than evaluating peer names or attachment
text as shell input.

The native Arch package is not published yet. Building from the source checkout
is the current fallback. The plugin never downloads or installs the binary.
