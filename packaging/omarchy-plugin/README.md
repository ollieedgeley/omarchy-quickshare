# Omarchy Quick Share plugin

This is the Omarchy shell plugin for the native `omarchy-quickshare` binary.
It adds a Quick Share icon to the bar and reports whether the installed binary
supports the plugin's local control protocol.

The current development build does not discover peers or transfer attachments.
The panel says so rather than displaying invented devices or progress.

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
