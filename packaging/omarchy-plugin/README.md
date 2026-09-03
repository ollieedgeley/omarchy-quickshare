# Omarchy Quick Share plugin

This is the Omarchy shell plugin for the native `omarchy-quickshare` binary.
It adds a Quick Share icon to the bar. The panel reads the native endpoint's
peer and transfer state, lets the user choose an outbound peer, handles inbound
consent, and provides transfer progress and cancellation controls.

The native binary also has an explicit simulated-peer mode for exercising both
directions without a physical phone. Normal service startup does not enable it.

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

The native Arch package is not published yet. Building from the source checkout
is the current fallback. The plugin never downloads or installs the binary.
