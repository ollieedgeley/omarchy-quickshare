# Omarchy Quick Share

Account-free Quick Share for Omarchy. One native daemon, one CLI, and a
separate shell plugin. The daemon can search for LAN peers, queue text,
URLs, files, and folders, pin one peer, take inbound consent, and move
file bytes on LAN. Folders are zipped before they are sent.

The plugin never downloads or installs the binary. The native Arch package
is not published. Physical-phone compatibility is not a development claim.

## Native install

User-local. No root.

```sh
make install-local
```

That builds the locked release binary, installs it to `~/.local/bin`,
writes `omarchy-quickshare.service` under `~/.config/systemd/user`, copies
a default config to `~/.config/omarchy-quickshare/config.toml` if that
file is missing, then reloads, enables, and starts the user service. The
unit runs `omarchy-quickshare daemon` and restarts after failures.

```sh
make install-local-simulation
make uninstall-local
```

Simulation starts `daemon --simulate` with fake peers.
`make install-local` restores the normal service. Uninstall stops and
removes the user unit and binary. It leaves the config file in place.

Quick Share discovers and advertises peers over mDNS on UDP port `5353`, then
accepts inbound phone transfers on TCP port `53318`. If a host firewall is
active, allow both ports only from the trusted LAN. For UFW:

```sh
sudo ufw allow from <lan-subnet> to any port 5353 proto udp
sudo ufw allow from <lan-subnet> to any port 53318 proto tcp
```

Put `~/.local/bin` on `PATH` if it is not already.

Local release artifacts, still unpublished:

```sh
make release-native
make release-arch
make release
```

`make release-native` writes the stripped binary, unit, default config,
license, and checksums under `dist/native`. `make release-arch` depends on
that native tree and writes a pacman package plus `PKGBUILD` under
`dist/arch`. `make release` runs native, source, sparse, Arch, then plugin
export. None of these publish a repository.

## Plugin

The plugin lives in `packaging/omarchy-plugin`. It is a bar widget. It
calls the installed binary through the CLI and shows missing, incompatible,
and stopped-service states as distinct messages.

Export a local Git repository after plugin sources are committed:

```sh
make plugin-export
omarchy plugin add "file://$PWD/dist/omarchy-plugin" --enable --yes
```

`omarchy plugin add` clones and validates. It does not run an installer.
This tree does not name a published plugin URL.

## Source build

One allowlist, `tools/release/source-allowlist.json`, drives both lean
routes. Runtime artifacts are the locked Cargo workspace, crates,
generated code, README, systemd unit, default config, and license. Tests,
tools, upstream trees, and caches stay out.

```sh
make release-source
make release-sparse
```

`make release-source` writes
`dist/source/omarchy-quickshare-source-<commit>.tar.gz` and proves a
clean extract plus locked build. Unpack it yourself with:

```sh
mkdir -p /tmp/omarchy-quickshare-src
tar -xzf dist/source/omarchy-quickshare-source-*.tar.gz \
  -C /tmp/omarchy-quickshare-src
cd /tmp/omarchy-quickshare-src
cargo build --release --locked --package omarchy-quickshare
```

`make release-sparse` materializes `dist/sparse` from this checkout with
the same path set. From a Git remote, the same allowlist is:

```sh
git clone --filter=blob:none --sparse --depth 1 \
  <repository> omarchy-quickshare-src
cd omarchy-quickshare-src
git sparse-checkout init --no-cone
git sparse-checkout set \
  Cargo.lock Cargo.toml LICENSE README.md rust-toolchain.toml \
  crates/app crates/core/connections crates/core/control \
  crates/core/crypto crates/core/sharing crates/core/wire \
  crates/platform/bluez crates/platform/network \
  crates/platform/storage \
  packaging/systemd/omarchy-quickshare.service \
  packaging/systemd/omarchy-quickshare.toml
cargo build --release --locked --package omarchy-quickshare
```

Replace `<repository>` with a clone URL you actually have. This project
does not publish one. A full checkout can run the same Cargo command.

## CLI

The CLI talks to `$XDG_RUNTIME_DIR/omarchy-quickshare/control.sock`. Start
the user service first.

Submit one argument directly, or use the explicit `send` command. An existing
file or directory is a file share. Directories are zipped first. `http://` or
`https://` is a URL. Anything else is text.

```sh
omarchy-quickshare "hello from Omarchy"
omarchy-quickshare https://example.test/share
omarchy-quickshare ./note.txt
omarchy-quickshare send ./photos
```

Successful submits print `Share N queued.` If a peer is pinned, the share
selects that peer. If not, discovery starts and you pick one. `--peer` keeps
clipboard content and the clicked recipient in one request.

```sh
omarchy-quickshare discover start
omarchy-quickshare send --peer pixel-8 "current clipboard"
omarchy-quickshare peer pin galaxy-tab
omarchy-quickshare share select 1 pixel-8
omarchy-quickshare discover stop
```

Live LAN file transfer starts after a peer is selected and has a LAN route.
Simulated mode can drive consent without that step.

Receive:

```sh
omarchy-quickshare visibility open
omarchy-quickshare share accept 1
omarchy-quickshare share reject 1
omarchy-quickshare visibility close
```

Cancel or clear a finished share:

```sh
omarchy-quickshare share cancel 1
omarchy-quickshare share dismiss 1
```

Status:

```sh
omarchy-quickshare protocol-version
omarchy-quickshare health
omarchy-quickshare status --json
```

`protocol-version` prints `3`. That is the only control protocol this tree
speaks. `status --json` writes one versioned envelope. A transferring file
looks like:

```json
{
  "response": {
    "type": "snapshot",
    "snapshot": {
      "active_share": {
        "attachment": { "name": "note.txt", "size_bytes": 4, "type": "file" },
        "direction": "outbound",
        "id": 1,
        "id_string": "1",
        "medium": "wifi_lan",
        "phase": "transferring",
        "remaining_seconds": 12,
        "total_bytes": 4,
        "transferred_bytes": 1
      },
      "discovery": "idle",
      "visibility": "closed"
    }
  },
  "version": 3
}
```

`id_string` is the lossless share id. `medium`, `remaining_seconds`,
`terminal_reason`, and `recovery_guidance` appear when the daemon has them.
`verification_code` appears while consent is open.

`daemon` and `daemon --simulate` belong to the service, not daily use.
Hidden `simulate` subcommands work only when the service was started with
`--simulate` and `OMARCHY_QUICKSHARE_ALLOW_SIMULATION=1`.

### Daemon diagnostics

The installed release daemon supports debug logging. Use a user-service drop-in
to enable it on the same daemon that normally handles shares:

```sh
systemctl --user edit --drop-in=quickshare-debug.conf omarchy-quickshare.service
```

Add:

```ini
[Service]
Environment=RUST_LOG=omarchy_quickshare=debug
```

Then reload the unit, restart it, and follow its stderr in the journal:

```sh
systemctl --user daemon-reload
systemctl --user restart omarchy-quickshare.service
journalctl --user -u omarchy-quickshare.service -f
```

This changes the log level of the installed service. It does not start a second
daemon or replace the release binary with a Rust debug build. `--log-level`
accepts `error`, `warn`, `info` (default), `debug`, or `trace`, but `RUST_LOG`
overrides that CLI setting. `RUST_LOG=omarchy_quickshare=debug` includes the
`omarchy_quickshare::protocol` target.

Debug logs record privacy-safe transitions through discovery, connection and
UKEY2, paired-key exchange, introduction, consent, payload transfer, terminal
outcome, and cleanup. A `connection_id` correlates events before a share exists;
a `share_id` is added later. Stable fields such as `operation`, `outcome`, and
`reason` describe failures. When available, `io_error_kind` names the local I/O
classification and `disconnect_origin` distinguishes a local close, an explicit
disconnection frame, stream EOF, a truncated frame, or a connection event.
Those origins describe what the daemon observed. A proprietary peer may
disconnect without disclosing its internal reason.

Rejected Connections frames also report `frame_type_present`, the received
`frame_type_code` when present, and `frame_type` as the known protobuf enum
name, `unrecognized` for an unknown number, or `missing` for an absent field.

`locally_written` and `staged` report local progress, not successful delivery.
Only a terminal `completed` outcome reports completion. `trace` adds per-frame,
chunk, and keepalive metadata when debug is not enough. Neither level logs
keys, verification codes, identities, addresses, filenames, paths, URLs, or
payload contents.

To restore the default log level, remove the dedicated drop-in, reload, and
restart:

```sh
rm "${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/omarchy-quickshare.service.d/quickshare-debug.conf"
systemctl --user daemon-reload
systemctl --user restart omarchy-quickshare.service
```

For an optional foreground run, stop the user service first so only one daemon
owns discovery and the socket. Start the service again after the foreground
daemon exits:

```sh
systemctl --user stop omarchy-quickshare.service
omarchy-quickshare daemon --log-level debug
# After exiting the foreground daemon:
systemctl --user start omarchy-quickshare.service
```

## Config and downloads

User config is `~/.config/omarchy-quickshare/config.toml`, or
`$XDG_CONFIG_HOME/omarchy-quickshare/config.toml`. Missing files use
defaults. Unknown keys are rejected.

Documented keys: `device_name`, `receive_directory`, `pinned_peer_id`,
`discovery_timeout_secs`, `visibility_timeout_secs`,
`transfer_timeout_secs`. `device_name` overrides the system hostname advertised
to nearby devices.

Default receive directory is `~/Downloads/omarchy-quickshare`. Incomplete
transfers stage a hidden file there and drop it if the share does not
commit.

Live inbound LAN saves files under `receive_directory`. Text and URL
payloads complete on the same path; their values appear in
`status --json`. Simulated inbound commands remain for local UI work
without a peer.

## Hyprland

This project does not edit your Hyprland config. Add binds yourself.

```conf
bindd = SUPER SHIFT, S, Quick Share paste, exec, sh -c 'value=$(wl-paste --type text/uri-list --no-newline 2>/dev/null || wl-paste --no-newline); omarchy shell io.github.ollieedgeley.omarchy-quickshare paste "$value"'
bindd = SUPER SHIFT, Q, Quick Share panel, exec, omarchy shell io.github.ollieedgeley.omarchy-quickshare open
```

Paste sends clipboard text, a URL, or one copied file or folder. If no
peer is pinned, the panel opens so you can choose one. The panel-open
bind does not send. You can also click .

## Evidence

Automated, no phone:

- `make test-local-install` for the user-local binary, config, and unit
- `make test-plugin-release` for the exported plugin and native status
  states
- `make test-source-build` for allowlist path sets and clean-build
  closure
- `make test-native-release` for native and Arch artifact contracts
- `make test-contracts` and the app process tests for CLI and control
  behavior
- `make test-rust-lan` for encrypted LAN file bytes against the
  Google-derived peer image

`make verify` never talks to a physical phone. Treat a phone run as
optional release evidence, not as a substitute for those gates.

Privacy-safe manual smoke, simulated first:

1. `make install-local-simulation`
2. Send dummy text, `https://example.test/share`, a throwaway file, and a
   throwaway folder.
3. Pin one fake peer, then send again. Try `share select` when nothing is
   pinned.
4. Offer inbound text, URL, and `note.txt` with the simulate commands, then
   accept or reject.
5. Cancel mid-transfer with `share cancel`. Dismiss terminal shares.
6. Read `status --json`. Keep `verification_code` off anything you share.
   Do not use real names, photos, or tokens.

Optional real-device pass after LAN tests are green: dummy files both
ways, inbound file to `~/Downloads/omarchy-quickshare`, PIN match on both
ends, reject, cancel, and an interrupted copy that leaves no staging file
behind. Record only the diagnostic fields above. That pass is not
publication and not compatibility.

## Development

```sh
make setup
make help
```

Hooks and quality gates are local. There is no hosted CI.

Start with [CONTEXT.md](CONTEXT.md). Task-specific links are in
[AGENTS.md](AGENTS.md).
