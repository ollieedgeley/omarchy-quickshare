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
selects that peer. If not, discovery starts and you pick one.

```sh
omarchy-quickshare discover start
omarchy-quickshare peer pin galaxy-tab
omarchy-quickshare share select 1 pixel-8
omarchy-quickshare discover stop
```

Live LAN file transfer starts with `share select` once the peer has a LAN
route. Simulated mode can drive consent without that step.

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

`protocol-version` prints `2`. That is the only control protocol this tree
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
  "version": 2
}
```

`id_string` is the lossless share id. `medium`, `remaining_seconds`,
`terminal_reason`, and `recovery_guidance` appear when the daemon has them.
`verification_code` appears while consent is open.

`daemon` and `daemon --simulate` belong to the service, not daily use.
Hidden `simulate` subcommands work only when the service was started with
`--simulate` and `OMARCHY_QUICKSHARE_ALLOW_SIMULATION=1`.

Development daemon:

```sh
omarchy-quickshare daemon --log-level debug
journalctl --user -u omarchy-quickshare.service -f
```

`--log-level` is `error`, `warn`, `info` (default), `debug`, or `trace`.
`RUST_LOG` overrides that choice. Logs are compact lines on stderr so the
journal captures them.

## Config and downloads

User config is `~/.config/omarchy-quickshare/config.toml`, or
`$XDG_CONFIG_HOME/omarchy-quickshare/config.toml`. Missing files use
defaults. Unknown keys are rejected.

Documented keys: `receive_directory`, `pinned_peer_id`,
`discovery_timeout_secs`, `visibility_timeout_secs`,
`transfer_timeout_secs`.

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
