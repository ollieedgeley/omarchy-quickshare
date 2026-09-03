# Project structure

Research date: 2026-09-02

## Decision

Use a virtual Cargo workspace with stable crates for wire formats, cryptography, Connections, Sharing, local control, Linux adapters, and the executable. Keep test infrastructure, upstream sources, tools, and packaging outside runtime crates.

The top-level directories and crate ownership defined here are permanent. Growth happens by adding a child module inside its existing owner. A module starts as `foo.rs`; when it grows, keep that interface file in place and add implementation files under `foo/`. Rust supports this layout directly and recommends descriptive module filenames instead of many `mod.rs` files in the [module reference](https://doc.rust-lang.org/stable/reference/items/modules.html#module-source-filenames).

No plan can predict every future product decision. The realistic promise is stronger than picking one large `src/` directory: existing concerns do not move when a known feature grows. A genuinely new product concern may add a crate, but it may not rearrange existing crates or reverse their dependency direction.

## Evidence behind the shape

Cargo workspaces share one lockfile, target directory, dependency declarations, lint policy, and build profiles. They also support package-targeted commands, which the pre-commit impact selector needs. A virtual workspace with resolver 3 makes every package an explicit member and lets root Cargo commands cover all members. Each member must opt into `[workspace.lints]` with `[lints] workspace = true`. See the official [workspace reference](https://doc.rust-lang.org/cargo/reference/workspaces.html).

Cargo compiles every file directly under a package's `tests/` directory as a separate crate. The Cargo reference recommends one integration-test entry file with child modules when a suite becomes large, because separate integration crates add compilation and serial execution cost. The proposed suite layout follows that advice. See [Cargo targets](https://doc.rust-lang.org/cargo/reference/cargo-targets.html#integration-tests).

Google's source tree shows what happens without early ownership. At the pinned Nearby revision, `connections/implementation` has 89 direct files, its `mediums` directory has 64, and `sharing` has 118. The counts come from Google's [tree at `5885319`](https://api.github.com/repos/google/nearby/git/trees/588531995decf09500870ed4d2e1ac6740a3e338?recursive=1). Its behavior still gives us the right durable concerns: offline frames, authentication, endpoint channels, payloads, bandwidth upgrades, media, advertisements, and incoming sharing sessions. We keep those concerns but reject the flat folders.

Omarchy imposes a separate distribution constraint. A third-party plugin is a Git repository with `manifest.json` at its root. `omarchy plugin add` clones the whole repository, validates it, runs no install hook, and never asks for sudo. See the pinned official [plugin manual](https://github.com/omacom/omarchy/blob/b71dcad96e9d0b2962b7d225828a5cb6000ad720/manual/32-shell-plugins.md) and [installer source](https://github.com/omacom/omarchy/blob/b71dcad96e9d0b2962b7d225828a5cb6000ad720/bin/omarchy-plugin-add). The installer uses a normal Git clone, which retrieves repository history unless shallow options are supplied; see the official [`git clone` reference](https://git-scm.com/docs/git-clone). A release branch inside the source repository would not solve the size problem. The source workspace must publish a small plugin repository separately from the native package.

## Stable repository map

Directories shown below are reserved locations, not instructions to create empty placeholders.

```text
.
├── AGENTS.md
├── CONTEXT.md
├── Cargo.lock
├── Cargo.toml
├── Makefile
├── README.md
├── rust-toolchain.toml
├── crates/
│   ├── app/
│   ├── core/
│   │   ├── connections/
│   │   ├── control/
│   │   ├── crypto/
│   │   ├── sharing/
│   │   └── wire/
│   └── platform/
│       ├── bluez/
│       ├── network/
│       └── storage/
├── docs/
│   ├── adr/
│   ├── architecture/
│   ├── operations/
│   ├── policy/
│   └── research/
├── fuzz/
│   ├── control/
│   ├── crypto/
│   ├── sharing/
│   └── wire/
├── packaging/
│   ├── arch/
│   ├── omarchy-plugin/
│   └── systemd/
├── rules/
│   └── ast-grep/
├── tests/
│   ├── environments/
│   │   ├── android/
│   │   ├── bluez/
│   │   ├── network/
│   │   ├── oracle/
│   │   └── proxies/
│   ├── fixtures/
│   │   ├── connections/
│   │   ├── control/
│   │   ├── crypto/
│   │   ├── sharing/
│   │   ├── storage/
│   │   └── wire/
│   ├── suites/
│   │   ├── android/
│   │   ├── bluetooth/
│   │   ├── contracts/
│   │   ├── dbus/
│   │   ├── interop/
│   │   ├── network/
│   │   ├── storage/
│   │   └── system/
│   └── support/
├── tools/
│   ├── codegen/
│   ├── gates/
│   ├── hooks/
│   ├── oracle/
│   ├── setup/
│   └── release/
└── upstream/
    ├── google/
    │   ├── nearby/
    │   └── ukey2/
    └── sources.toml
```

Generated output belongs in `target/`, `dist/`, or `reports/`. Downloaded source trees, Android images, VM disks, packet captures, and fuzz artifacts belong in an ignored `.cache/`. They never enter the plugin repository.

## Runtime crate ownership

Directory names stay short. Cargo package names carry the public namespace.

| Directory                 | Cargo package            | Sole owner of                                                                                                  |
| ------------------------- | ------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `crates/core/wire`        | `quickshare-wire`        | generated Google messages, bounded codecs, length framing, wire-version dispatch                               |
| `crates/core/crypto`      | `quickshare-crypto`      | UKEY2, D2D keys, authentication strings, encryption, MACs, sequence enforcement                                |
| `crates/core/connections` | `quickshare-connections` | endpoint sessions, channels, payload transfer, keepalives, cancellation, medium selection, upgrades            |
| `crates/core/sharing`     | `quickshare-sharing`     | advertisements, discovery, paired-key flow, introductions, consent, attachments, inbound and outbound outcomes |
| `crates/core/control`     | `quickshare-control`     | versioned same-user command and event messages used by the daemon, CLI, and plugin                             |
| `crates/platform/bluez`   | `quickshare-bluez`       | BlueZ D-Bus, BLE advertising, GATT, L2CAP, RFCOMM, Classic profiles, adapter lifecycle                         |
| `crates/platform/network` | `quickshare-network`     | DNS-SD, TCP, NetworkManager, hotspot, Wi-Fi Direct, network cleanup                                            |
| `crates/platform/storage` | `quickshare-storage`     | safe receive staging, outbound source opening, path validation, quota checks, atomic completion, cleanup       |
| `crates/app`              | `omarchy-quickshare`     | composition, configuration, daemon lifecycle, CLI dispatch, Unix socket, diagnostics                           |

Cryptography, BlueZ, networking, storage, and control remain separate because they have distinct dependencies, security reviews, simulator environments, and targeted gates. Do not create a crate per medium, attachment type, protocol message, or state-machine phase. Those are modules inside the owner above.

The app package produces one `omarchy-quickshare` binary. `src/main.rs` only parses process-level input and calls the library. Daemon and CLI subcommands share the same composition code rather than becoming separate binaries.

Do not create production directories or crates named `common`, `shared`, `utils`, `misc`, `helpers`, or `models`. A type lives with the behavior that gives it meaning. Other crates import it from that owner.

## Dependency direction

An arrow means "may depend on."

```text
crypto       -> wire
connections  -> wire, crypto
sharing      -> wire, connections
control      -> sharing
bluez        -> connections
network      -> connections
storage      -> sharing
app          -> control, sharing, bluez, network, storage
```

Core crates never depend on platform crates or the app. Platform crates do not depend on one another. Only the app composes adapters. The workspace must reject dependency cycles and any edge outside this allowlist.

The `connections` and `sharing` interfaces are the main test seams. Production media adapters and test fakes satisfy the internal media interfaces. Callers do not learn BlueZ, NetworkManager, protobuf, or cryptographic details.

## Module growth pattern

Each crate starts with a small `lib.rs` that declares modules and re-exports only its intended interface. It does not collect implementation code. Use the same growth pattern everywhere:

```text
src/
├── lib.rs
├── session.rs
└── session/
    ├── handshake.rs
    ├── keepalive.rs
    └── shutdown.rs
```

The initial `session.rs` remains the interface when the implementation expands. Crate roots keep stable re-exports so callers use the owning crate's interface rather than internal file paths. Files move only if their owner was wrong, which is an architecture defect rather than normal growth.

Expected module homes are:

- `wire`: `connections`, `sharing`, `ukey2`, `secure_message`, `framing`, `limits`, and `generated`
- `crypto`: `handshake`, `keys`, `authentication`, `secure_channel`, and `sequence`
- `connections`: `session`, `channel`, `payload`, `keepalive`, `medium`, `upgrade`, and `event`
- `sharing`: `advertisement`, `discovery`, `visibility`, `paired_key`, `receive`, `send`, `attachment`, `consent`, and `event`
- `bluez`: `adapter`, `advertisement`, `gatt`, `l2cap`, `classic`, and `monitor`
- `network`: `dns_sd`, `lan`, `hotspot`, `wifi_direct`, and `network_manager`
- `storage`: `path`, `source`, `quota`, `staging`, `commit`, and `cleanup`
- `control`: `request`, `response`, `event`, `codec`, and `version`
- `app`: `cli`, `daemon`, `config`, `runtime`, `socket`, and `diagnostics`

Versioned formats begin in version-named leaf files such as `advertisement/v1.rs`, `advertisement/v2.rs`, and `connections/v1.rs`. A new wire version adds a sibling and dispatch entry. It does not rename the existing implementation.

## Test structure

`tests/support` is a `publish = false` workspace package used only through dev-dependencies. Organize it by domain first, then support role. Builders, factories, fixtures, fakes, stubs, and mocks retain those words in their type names. No runtime crate depends on it.

Every directory under `tests/suites/` is a `publish = false` Cargo package with one explicit integration-test target. Its entry file is `tests/suite.rs`; cases live under `tests/suite/`. This preserves targeted package gates without making every case a separately compiled integration crate.

Suite ownership is stable:

- `contracts` tests public crate and local-control interfaces
- `interop` drives the pinned C++ oracle and byte-exact reference cases
- `dbus` tests BlueZ and NetworkManager service behavior on private buses
- `bluetooth` owns btvirt, hciemu, Bumble, RootCanal, and Netsim routes
- `network` owns Turmoil, Toxiproxy, hwsim, LAN, hotspot, and Wi-Fi Direct routes
- `android` owns repeatable AVD and Cuttlefish black-box routes for both transfer directions
- `storage` owns hostile paths, quotas, interruption, cleanup, and atomic completion
- `system` owns the built binary, Unix socket, service lifecycle, and packaging contracts

Fixtures are grouped by the domain that interprets them, then by scenario. Each generated fixture has provenance and expected semantic meaning beside it. Environment definitions contain orchestration only; assertions stay in suites. Large downloaded environments remain in `.cache/`.

The Android probe keeps Gradle's generated dependency-verification metadata at `tests/environments/android/probe/gradle/verification-metadata.xml`. Gradle requires this as one canonical file, so the structure gate excludes it from authored-file metrics while contract tests verify its checksums and dependency lock. Regenerate it only through the pinned probe toolchain.

Each `fuzz/<domain>` directory is an independent cargo-fuzz package and follows cargo-fuzz's standard `fuzz_targets/` layout. The official [Rust Fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz/tutorial.html) defines that layout. Keep fuzz packages outside the normal workspace members so ordinary Cargo commands do not select libFuzzer targets accidentally.

## Tools and upstream inputs

`tools/oracle` owns the small C++ reference executable and its language-neutral command protocol. `tools/codegen` owns pinned protobuf generation. `tools/gates` owns quality-gate orchestration, `tools/hooks` owns the implementation behind the tracked Husky entry points, `tools/setup` owns reproducible development-tool provisioning, and `tools/release` produces local release artifacts. The root Makefile remains the only public project task interface.

`upstream/google` contains only the exact source files and licenses required to generate or audit the Rust implementation. `sources.toml` records repository URLs, commits, paths, and hashes. Do not vendor complete Nearby, UKEY2, BlueZ, Android, or emulator repositories. Fetch their pinned trees into `.cache/` for oracle and simulation gates.

Generated Rust is isolated under `crates/core/wire/src/generated/`. Project code may import it only through hand-written wire modules. Generated files are exempt from the line limit, but no project-authored logic belongs in them. Cargo build scripts may write only to `OUT_DIR`, as required by the official [build-script reference](https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script). Deliberate regeneration uses the local codegen target and produces a reviewable committed diff.

## Packaging and publication

The source repository will produce three user-facing artifacts:

1. A native install artifact containing the stripped `omarchy-quickshare` binary, its systemd user unit, license, and default configuration. The current target is an Arch package; future release automation may also publish a prebuilt binary artifact.
2. An allowlisted source-build bundle for the native binary.
3. A small Git repository exported from `packaging/omarchy-plugin/`. Its root contains `manifest.json`, `release.json`, QML entry points, assets, license, and README. It contains no Rust source, binary, test data, symlink, or installer.

The source-build bundle is the permanent fallback for native artifacts. It contains only the locked Rust toolchain and Cargo inputs, runtime crates, committed generated code, build-required configuration, notices, and source-build instructions. It excludes tests, fuzz targets, project tools, upstream reference sources, protocol oracles, simulators, virtual-machine assets, Android tooling, reports, and caches.

The runtime workspace must be build-closed so the bundle does not need a rewritten Cargo graph. Runtime package manifests may refer only to files and path dependencies included in the bundle. Test-only packages depend on runtime crates, never the reverse. Build scripts and code generation used by an ordinary release build may not read from `tests/`, `tools/`, `upstream/`, `.cache/`, or installed test tools. Generated protocol sources required to compile remain committed and reproducible.

Local release tooling must verify the bundle by extracting it into an empty directory and running the documented locked release build. That check starts without repository-relative files or test tools and proves the resulting binary's version. A full source checkout must support the same locked Cargo build. The same allowlist drives a documented Git partial-clone and sparse-checkout route, so a user can fetch the buildable runtime tree without downloading test blobs. A contract test materializes both lean routes, compares their path sets, and builds each in isolation. The lean routes save download size; they are not a second implementation or a weaker build path.

The plugin calls the installed binary through its CLI and local-control interface. If the package is missing or incompatible, the plugin reports that state. It does not download or install executables from QML.

The plugin checks that the binary exists, is executable, and supports the control-protocol range declared in `release.json`. Missing, incompatible, and unavailable-runtime states produce distinct messages with the supported package and source-build routes. Installing the plugin through Omarchy remains independent from installing the native binary.

`release.json` records the source commit, plugin version, compatible control-protocol range, native artifact version, and checksums for published native and source-build artifacts. The exported repository is generated from an allowlist and never edited by hand.

Local release tooling verifies the native artifact, source-build bundle, and plugin export, validates the plugin with `omarchy plugin validate`, and records the source commit used to build them. Publication remains local for now. Future hosted release automation must call the same versioned local targets and may publish prebuilt artifacts, but it does not replace the source-build fallback.

## Directory budgets

The future file-count gate must enforce these rules in addition to the existing per-file line limits:

- no maintained source, test, tool, rule, or documentation directory contains more than 12 direct files
- no maintained directory contains more than 12 direct child directories unless it is an explicit index such as `crates`, `tests/suites`, or `tests/fixtures`
- `lib.rs`, `main.rs`, and test-suite entry files route to modules; they do not become implementation dumps
- generated, vendored, cache, and artifact directories are excluded from the count only when their path and provenance are declared
- crossing a budget adds a child under the existing owner; it never creates numbered parts or a catch-all directory

When this gate is implemented, the same change must add its narrow Make command to `AGENTS.md` under `Fast feedback gates`.

## Predicted additions

| Future change                      | Stable addition point                                                        |
| ---------------------------------- | ---------------------------------------------------------------------------- |
| New attachment type                | `sharing/attachment/<type>.rs` plus sharing fixtures                         |
| New advertisement or frame version | versioned leaf under `wire` and a matching interop case                      |
| New Bluetooth behavior             | child module in `bluez` and case in the Bluetooth suite                      |
| New NetworkManager or IP medium    | child module in `network` and case in the network suite                      |
| Account or contact visibility      | new core crate; existing account-free sharing crates remain in place         |
| New outbound attachment kind       | `sharing/send/` and matching sharing fixtures                                |
| Another operating system           | sibling under `crates/platform`; core crates remain unchanged                |
| New simulator                      | environment under `tests/environments` and an owning suite module            |
| Richer Omarchy UI                  | QML and assets in `packaging/omarchy-plugin`; daemon crates remain unchanged |

This is the test for the structure: every already-foreseeable feature has one owner and one growth path, without a future move of unrelated files.
