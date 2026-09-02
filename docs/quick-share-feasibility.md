# Android Quick Share endpoint feasibility

Research date: 2026-09-01

## Short answer

A small Omarchy endpoint is feasible, with a catch. A same-network implementation that receives supported attachments and sends files is a reasonable project. Matching the reliability and device coverage of Google's Windows client is not.

The useful first version would be one native daemon and one CLI, delivered as a single package:

- Android to Omarchy for files, text, and URLs
- Omarchy to Android for files
- same Wi-Fi or Ethernet LAN
- inbound visibility as an "Everyone" device and outbound discovery of public peers
- a four-digit PIN plus an explicit local or remote consent decision
- mDNS discovery for the first same-LAN spike
- BlueZ listener and initiator paths in the hardened alpha if testing shows that target phones leave Wi-Fi during discovery
- downloads confined to a dedicated directory

It would not need ADB, the Android SDK, Java, an Android companion app, or a Google account. Omarchy already installs BlueZ, Avahi, NetworkManager, and the components needed for desktop notifications. See its [base package list](https://github.com/basecamp/omarchy/blob/4d017913d06f715da9d960021861cf535e4f15aa/install/omarchy-base.packages) and [service setup](https://github.com/basecamp/omarchy/blob/4d017913d06f715da9d960021861cf535e4f15aa/install/config/enable-services.sh).

My recommendation is to prove interoperability in both directions by adapting the active [`asulwer/rquickshare`](https://github.com/asulwer/rquickshare) Rust protocol core into a separate GPL-3.0 endpoint binary. Test the newer BLE/GATT receiver branch described below at the same time. Then decide whether to keep that license or replace the core with a smaller permissively licensed implementation. Do not build a Tauri, GTK, or Qt application. Omarchy already has a notification UI and system tray.

## What Google makes available

Google's user documentation lists Android, ChromeOS, and selected Windows PCs as Quick Share platforms. It does not offer a Linux client or Linux protocol SDK. The Windows client requires Bluetooth plus Wi-Fi or Ethernet, normally on the same network, and Google says transfers are encrypted. It also describes the PIN check and the account-backed "Contacts" and "Your devices" visibility modes. See [Quick Share between Android and Windows](https://support.google.com/android/answer/13801258?hl=en) and [Quick Share on Android](https://support.google.com/android/answer/15728591?hl=en).

Nearby Connections is public, but it is not a Quick Share compatibility API. Google presents it as an app-to-app API where both apps use the same service ID. The supported client platforms are Android and iOS. See the [Nearby overview](https://developers.google.com/nearby/overview) and [advertising and discovery guide](https://developers.google.com/nearby/connections/android/discover-devices). Calling that API from a custom phone app would create a new sharing system, not make Linux appear in Android's built-in Quick Share sheet.

There is still a substantial amount of first-party source to work from:

- Google's [`nearby`](https://github.com/google/nearby) repository includes the Connections transport, Quick Share service code, and protobuf definitions under Apache-2.0. Google labels the repository as not officially supported.
- Its [Connections README](https://github.com/google/nearby/blob/main/connections/README.md) says the core builds on Linux, but Linux has no implemented transport mediums. That rules out using it as a ready-made Linux engine.
- The Quick Share attachment and control messages are published in [`sharing/proto/wire_format.proto`](https://github.com/google/nearby/blob/main/sharing/proto/wire_format.proto). The lower-level connection, payload, keepalive, and bandwidth-upgrade messages live in [`connections/implementation/proto/offline_wire_formats.proto`](https://github.com/google/nearby/blob/main/connections/implementation/proto/offline_wire_formats.proto).
- UKEY2, Google's authenticated key exchange used by Nearby Connections, has an Apache-2.0 [reference implementation and protocol description](https://github.com/google/ukey2/blob/master/README.md).

This is enough to make an interoperable client, but there is no stable Quick Share protocol specification or compatibility promise.

## How the lean path works

The LAN protocol has been implemented independently by NearDrop, RQuickShare, Packet, QNearbyShare, and newer experimental clients. NearDrop's [protocol notes](https://github.com/grishka/NearDrop/blob/master/PROTOCOL.md) are the clearest readable account, while the Google protobuf and implementation sources remain the authority for message definitions.

For an Android to Omarchy transfer, Omarchy is the receiver and TCP server:

1. The daemon binds an arbitrary local TCP port and advertises DNS-SD service type `_FC9F5ED42C8A._tcp.local.`. The instance name and `n` TXT record carry an endpoint ID, device type, visibility, identity metadata, and optionally the visible device name. The service identifier is derived from `SHA256("NearbySharing")`. RQuickShare's [mDNS server](https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/mdns.rs#L133-L154) is a compact working example.
2. Android emits a BLE service advertisement while looking for targets. A receiver can listen for it and reannounce mDNS at the right moment. RQuickShare does this because an mDNS record that predates Android's scan is sometimes missed; see its [reannounce logic](https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/mdns.rs#L97-L108) and [BLE listener](https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/ble.rs). BLE is discovery help in this design. File bytes still travel over TCP on the LAN.
3. Android connects to the advertised port. Each protobuf packet has a four-byte big-endian length prefix.
4. The peers exchange Nearby Connections setup frames and run UKEY2. The common suite uses ephemeral P-256 ECDH and a SHA-512 commitment. UKEY2 derives an authentication string and a next-protocol secret from the shared secret and handshake transcript. Google's [UKEY2 README](https://github.com/google/ukey2/blob/master/README.md#deriving-the-authentication-string-and-the-next-protocol-secret) spells out the derivation and why visual comparison resists an active man-in-the-middle attack.
5. The next-protocol secret derives separate directional encryption and authentication keys. Subsequent messages use AES-256-CBC and HMAC-SHA256. A sequence number inside each encrypted device-to-device message prevents silent reordering.
6. Both sides exchange paired-key frames. A client without Google's account certificates reports `UNABLE`, so Android falls back to the four-digit authentication code. This is acceptable for a first release and avoids Google account and contact infrastructure.
7. Android sends an introduction with filenames, sizes, MIME types, and payload IDs. Omarchy shows the sender, contents, total size, and PIN, then sends accept or reject.
8. Accepted files arrive as encrypted payload chunks keyed by payload ID and offset. Application keepalives are required during long transfers. The receiver validates the declared size and offset, writes a temporary file, then atomically renames it after the last chunk.

The result is encrypted in transit. It is not account-authenticated. The PIN comparison plus local user approval is the trust check.

For an Omarchy to Android file transfer, the protocol roles reverse. Omarchy discovers an available Android peer, initiates the Connections handshake, creates the outgoing Sharing introduction, waits for the peer's consent, and streams the declared file payloads after acceptance. This uses the same wire, cryptography, payload, and medium layers, but it needs separate initiator state machines and tests. Passing the inbound flow does not prove outbound discovery, consent, cancellation, or payload sending.

There is now a second receiver-discovery path to account for. A documented [RQuickShare prototype branch](https://github.com/martinalderson/rquickshare/blob/feat/ble-receiver-connect-back/docs/BLE_RECEIVER_DISCOVERY.md) handles Pixel phones that disconnect from Wi-Fi while browsing after Google's AirDrop compatibility update. Linux advertises a connectable Quick Share receiver under BLE service `0xFEF3`, hosts the corresponding GATT service, accepts a Nearby "weave" data socket over BLE, completes the encrypted connection there, then offers a Wi-Fi LAN bandwidth upgrade so file bytes use TCP. This is much more than listening for a BLE beacon. It is still one native binary talking to BlueZ, but it adds a GATT server, packet reassembly, another socket transport, and a migration state machine. The branch reports working Android-to-Linux transfers and is the best available prototype for this specific regression, not a stable upstream feature.

## Reuse choices

| Option                                                                   | License                                             | Fit for a small Omarchy binary                                                                                                                                                                                                                                                                                                                                                                                                                        | Verdict                                                                                                                  |
| ------------------------------------------------------------------------ | --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Google's `nearby` C++ stack                                              | Apache-2.0                                          | Complete concepts and current protobufs, but the Linux build has no mediums and pulls in a large Bazel/C++ dependency graph                                                                                                                                                                                                                                                                                                                           | Reference source, not the implementation base                                                                            |
| `kidfromjupiter/nearby` Linux fork                                       | Apache-2.0                                          | Ports Google's Connections and Sharing stack to Linux, includes a [headless CLI](https://github.com/kidfromjupiter/nearby/blob/main/sharing/linux/nearby_sharing_cli.cc), and claims LAN, hotspot, and Bluetooth transfer against native Android Quick Share. It requires systemd, NetworkManager, and BlueZ 5.85 or newer                                                                                                                            | Strong full-stack experiment and protocol tracker, but large, young, and explicitly buggy                                |
| RQuickShare `rqs_lib`                                                    | GPL-3.0 through the repository                      | Rust, separates protocol logic from its Tauri UI, supports inbound and outbound transfers, mDNS, BLE, protobuf, and pure-Rust crypto. Its [manifest](https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/Cargo.toml) shows that the UI stack is not part of the core. Mainline stopped moving in June 2025, while [`asulwer/rquickshare`](https://github.com/asulwer/rquickshare) was active in July 2026 | Fastest route to a prototype, provided the new binary is GPL-3.0 and the core receives a security review                 |
| RQuickShare BLE receiver branch                                          | GPL-3.0                                             | Adds connectable `0xFEF3` advertising, a BlueZ GATT server, BLE weave socket, and BLE-to-TCP bandwidth upgrade; see its [technical notes](https://github.com/martinalderson/rquickshare/blob/feat/ble-receiver-connect-back/docs/BLE_RECEIVER_DISCOVERY.md)                                                                                                                                                                                           | Most relevant proof for current Pixel discovery, but experimental and considerably more stateful than LAN-only receiving |
| Packet                                                                   | GPL-3.0-or-later                                    | Maintained Linux UI that consumes a fork of `rqs_lib`, but adds GTK4, libadwaita, portals, D-Bus, and tray code; see its [Cargo manifest](https://github.com/nozwock/packet/blob/main/Cargo.toml)                                                                                                                                                                                                                                                     | Useful interoperability reference, too much UI for Omarchy                                                               |
| NearDrop                                                                 | Unlicense                                           | Small, proven receive-side logic and excellent protocol notes, but implemented for macOS in Swift                                                                                                                                                                                                                                                                                                                                                     | Good cross-check, not a Linux base                                                                                       |
| QNearbyShare                                                             | MIT                                                 | C++ receiver with Qt 6, Avahi, protobuf, and OpenSSL or Crypto++; its [README](https://github.com/vicr123/QNearbyShare) reports Android-to-Linux receiving                                                                                                                                                                                                                                                                                            | Permissive, but dated and heavier than a Rust daemon                                                                     |
| pyquickshare                                                             | MIT                                                 | Broad Linux transport reference using BlueZ and NetworkManager, including Bluetooth Classic and Wi-Fi Direct upgrades; see its [README](https://github.com/teaishealthy/pyquickshare)                                                                                                                                                                                                                                                                 | Valuable permissive reference, but Python and its runtime extensions do not meet the single-native-binary goal           |
| Fresh Rust implementation using Google's protobufs and UKEY2 description | Apache-2.0-compatible if written from those sources | Best long-term control and smallest dependency set                                                                                                                                                                                                                                                                                                                                                                                                    | Sensible after the compatibility spike, not the quickest proof                                                           |

RQuickShare's packaged GUI should not set expectations about footprint. Its current Debian archive is about 6.6 MB and the unstripped executable about 17.6 MB, but it dynamically loads WebKitGTK and GTK. Removing the web UI also removes that long native dependency chain. A daemon built from the protocol core should need only libc at the executable boundary plus the already-installed BlueZ D-Bus service at runtime. The Rust crypto, protobuf, mDNS, and async networking crates can be linked into the binary.

There is one licensing choice to make early. Reusing or modifying `rqs_lib` means distributing the endpoint under GPL-3.0 and providing corresponding source. Keeping it as a separate executable avoids combining GPL code into Omarchy's MIT-licensed shell, but distribution details still need a deliberate license review. A fresh implementation from Google's Apache-2.0 protobufs and UKEY2 source can remain permissive.

## Practical Omarchy architecture

```text
Android Quick Share endpoint
    ^  BLE, Bluetooth Classic, LAN, hotspot, or Wi-Fi Direct
    |  UKEY2 + encrypted protobuf payloads in either direction
    v
omarchy-quickshare user service
    |  Unix socket control API
    +--> omarchy-quickshare discover|send|accept|reject|status
    +--> Omarchy notification or picker with peer, files, size, PIN
    +--> inbound staging and bounded outbound source reads
```

The daemon should run as a systemd user service after the graphical session and network are available. It should expose a tiny local control socket rather than embedding UI. Omarchy's notification UI invokes the default libnotify action rather than rendering separate accept and reject buttons. Its own sender does provide a persistent `--exec` click command, so the notification can safely open a small Omarchy-native confirmation flow or terminal prompt. A single click is not enough for both choices. The best UI is a small shell overlay with two actions. The tray can later expose visibility and recent transfers.

Recommended process boundaries:

- `omarchy-quickshare daemon`: network-facing state machine, one active transfer per peer, bounded concurrency
- `omarchy-quickshare status|on|off|discover|send|accept|reject|cancel`: talks over a Unix socket with same-user permissions
- no root process, no listening system service, no shell parsing of remote names
- a fixed configurable TCP port and a narrow UFW rule. Omarchy's [firewall setup](https://github.com/basecamp/omarchy/blob/4d017913d06f715da9d960021861cf535e4f15aa/install/config/firewall.sh) defaults to denying incoming traffic and only opens LocalSend's TCP and UDP port. An ephemeral listener would be blocked unless the firewall were changed dynamically

Inbound visibility should default to off or a short explicit window. "Visible all the time" exposes a TCP parser and the machine name to every peer on the LAN. Outbound discovery starts only from an explicit user action and does not make Omarchy available for inbound shares.

## Security work required before shipping

The independent implementations prove feasibility, not production readiness. Network input includes protobuf lengths, nested frames, sender names, filenames, sizes, MIME types, payload IDs, offsets, and chunk bytes. Every field needs a limit and a state check.

In particular, RQuickShare must not be shipped unchanged. Its inbound code appends the sender-provided filename directly to the download path and later passes that result to `File::create`; see [filename handling](https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/inbound.rs#L825-L860) and [file creation](https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/inbound.rs#L1001-L1009). Absolute paths and `..` components must be rejected, not normalized after joining. Other release gates should include:

- write only under an opened download-directory file descriptor, with no symlink following
- create unpredictable temporary files with exclusive creation
- cap frame size, attachment count, per-file size, total size, and concurrent connections before allocation
- reject negative sizes, duplicate payload IDs, overlapping chunks, non-monotonic offsets, early last-chunk markers, and data past the declared length
- verify HMAC before parsing decrypted contents and enforce sequence numbers
- time out every handshake state and idle transfer
- compare available disk space before acceptance
- preserve partial files only when explicitly useful, otherwise remove them on failure
- open outbound source files without following symlinks, reject non-regular files, and detect replacement or size changes during transfer
- never interpolate device names, filenames, or text payloads into shell commands or notification markup
- fuzz the plaintext and encrypted frame decoders and test path handling separately

Google's own repository continues to receive transport and parser hardening. That is another reason to keep our first version on the simplest LAN transport and avoid bandwidth upgrades until the base state machine is audited.

The Apache-2.0 [`kidfromjupiter/nearby` fork](https://github.com/kidfromjupiter/nearby) is worth tracking during the spike. It is the closest public Linux port to Google's current stack and can test whether a discovery failure belongs to the old Rust implementation or to the LAN approach itself. Its own README warns that builds may fail, BLE advertisements are not cleaned up on exit, leaving it running may interfere with Bluetooth devices, Bluetooth Classic is slow, and existing-file handling is unfinished. It is evidence that fuller parity is possible, not release-ready code.

## Interoperability limits and recent drift

Same-LAN support works on many devices, but it cannot be advertised as universal Quick Share compatibility.

- Google's supported surface changes without a Linux compatibility contract. The open-source repository is explicitly unsupported, and Google's [AirDrop interoperability announcement](https://blog.google/products-and-platforms/platforms/android/quick-share-airdrop/) shows that discovery and transport behavior is still expanding.
- Account-backed "Contacts" and "Your devices" modes depend on public certificates, encrypted metadata keys, Google RPCs, and account state visible in Google's [sharing service implementation](https://github.com/google/nearby/blob/main/sharing/nearby_sharing_service_impl.cc). The account-free endpoint should use public discovery and manual PIN approval in both directions.
- Wi-Fi client isolation, VPNs, multicast filtering, and UFW can block mDNS or the TCP port. Google's own Windows troubleshooting tells users to use the same network and notes that managed networks may block device-to-device sharing.
- RQuickShare is a working proof, but its mainline last tagged release is [v0.11.5 from 2025-02-23](https://github.com/Martichou/rquickshare/releases/tag/v0.11.5) and its last source commit was in June 2025. The active fork should be the spike base. Current open reports show protocol drift, including [nameless 17-byte Android mDNS records](https://github.com/Martichou/rquickshare/issues/431) and a [Pixel 10 discovery failure after its AirDrop-compatible Quick Share update](https://github.com/Martichou/rquickshare/issues/425). These are field reports, not proof that all affected models fail, but they are exactly the sort of regression an unofficial endpoint must track.
- Google's current Pixel help says some devices may disconnect Wi-Fi while the Quick Share screen is open for AirDrop-compatible sharing; see [Pixel Quick Share help](https://support.google.com/pixelphone/answer/2781895?hl=en). A LAN-only target can disappear in that mode.
- Samsung uses vendor-specific behavior and may omit the clear-text device name. Test Samsung separately from Google Play Services Quick Share.
- Wi-Fi Direct, hotspot, Bluetooth Classic data, Wi-Fi Aware, and WebRTC are separate transports or bandwidth upgrades. Each supported route needs direction-specific role and fallback tests before it contributes to a compatibility claim.

The target support statement is: "Sends files to and receives supported content from Android Quick Share devices. Uses account-free public discovery and PIN confirmation. Available connection routes depend on the Linux and Android hardware."

## Estimated effort

The original estimates covered only Android-to-Omarchy receiving. They are not project estimates now that outbound file sharing is required. Estimate the bidirectional product only after the oracle proves outgoing discovery, initiator handshakes, peer consent, payload sending, cancellation, and cleanup over the required media.

## Proposed decision

Proceed with a narrow spike. The project is easy enough to justify that experiment because the difficult wire pieces already have working implementations and Omarchy already provides the Linux services. It is not easy enough to promise "Quick Share for Linux" before testing actual current phones.

The spike should answer five yes-or-no questions:

1. Can Omarchy and the pinned reference peer complete a 1 GB LAN transfer in each direction?
2. Do current Pixel and Samsung behavior have programmatic substitutes for peer discovery, consent, and payload verification in both directions?
3. Can BlueZ advertise and discover in both local roles without disturbing headphones, keyboards, or Omarchy's Bluetooth panel?
4. Can the endpoint pass a focused security review and fuzz run for inbound writes and outbound source reads?
5. Can every supported medium and upgrade pass initiator, responder, rejection, cancellation, fallback, and cleanup scenarios without a physical phone?

If those pass, an Omarchy-native endpoint is a good fit. If either direction lacks a reliable reference peer or simulator route, stop and close that verification gap before investing in UI.
