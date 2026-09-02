# Bidirectional Quick Share implementation baseline

Research date: 2026-09-02

This document defines the public implementation and verification baseline for a
Linux endpoint that sends to and receives from Android Quick Share. It does not
claim parity with every feature carrying the Quick Share name.

## Decision

The practical target is the local, account-free Nearby Sharing protocol in
Everyone visibility. The first complete product slice is file, plain text, and
URL transfer in both directions. It must cover the repository's confirmed local
connection paths, consent, cancellation, fallback, and cleanup. Google account
or contact trust, QR cloud sharing, NFC tap-to-share, and AirDrop interoperability
are separate compatibility projects.

There is enough public source to build this target, but no supported Google Linux
library or single simulator proves it. Google's current source says that its
Linux build has no medium implementations. The implementation therefore needs a
Rust protocol stack, Linux medium adapters, and several pinned reference layers.
The source is explicitly an unsupported Google product, not a compatibility
contract. See Google's [Nearby README](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/README.md)
and [Connections platform table](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/README.md).

The primary source pins for this baseline are:

- `google/nearby` at [`588531995decf09500870ed4d2e1ac6740a3e338`](https://github.com/google/nearby/commit/588531995decf09500870ed4d2e1ac6740a3e338),
  dated 2026-08-31.
- `google/ukey2` at [`10fc737aa901e873a3367e7e26b88eb01cd55d69`](https://github.com/google/ukey2/commit/10fc737aa901e873a3367e7e26b88eb01cd55d69),
  dated 2026-01-20.
- The experimental Linux fork `kidfromjupiter/nearby` at
  [`6887b0983200c6c8c29e614ea2633d13bf18315d`](https://github.com/kidfromjupiter/nearby/commit/6887b0983200c6c8c29e614ea2633d13bf18315d),
  dated 2026-08-01. This is a test candidate, not an authority.

## Exact transfer surface

Quick Share attachment families and Nearby Connections payload kinds are
different layers. The lower layer carries only `BYTES`, `FILE`, and `STREAM`.
The Sharing introduction refers to those payload IDs using five metadata
families. See the current [Sharing wire schema](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/proto/wire_format.proto)
and [Connections payload schema](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/proto/offline_wire_formats.proto#L163-L219).

| Sharing family    | Variants in the wire schema                                       | Current open desktop receiver   | Current open desktop sender | Project status                                                           |
| ----------------- | ----------------------------------------------------------------- | ------------------------------- | --------------------------- | ------------------------------------------------------------------------ |
| File              | unknown, image, video, Android app, audio, document, contact card | Yes                             | Yes                         | Required both ways                                                       |
| Text              | unknown, text, URL, address, phone number                         | Yes                             | Yes                         | Text and URL required both ways; preserve all subtype values             |
| Wi-Fi credentials | open, WPA-PSK, WEP, SAE, password and hidden flag                 | Yes                             | Yes                         | Protocol-compatible extension, test both ways before claiming support    |
| App bundle        | package plus one or more APK `FILE` payloads                      | Yes, mapped to file attachments | No                          | Receive-only in public source; outbound needs new behavior and an oracle |
| Stream            | description, package attribution and stream payload               | No open Sharing path            | No open Sharing path        | Not supported until both roles have an executable reference case         |

The current sender fills introductions only for file, text, and Wi-Fi credential
attachments in [`OutgoingShareSession::FillIntroductionFrame`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session.cc#L270-L324).
The receiver accepts file, app, text, and Wi-Fi credential metadata in
[`IncomingShareSession`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/incoming_share_session.cc).
An APK sent as ordinary file metadata is not the same contract as a split APK
`AppMetadata` bundle.

This reconciles the product requirement with older outbound-files-only notes:
file, plain text, and URL are now mandatory for both send and receive. A literal
claim of every schema attachment would additionally require app-bundle sending
and stream support in both roles.

## Discovery and connection paths

Quick Share uses service ID `NearbySharing`, point-to-point strategy, and fast
BLE UUID `0000fef3-0000-1000-8000-00805f9b34fb`. The current wrapper's advertise,
discover, and connect options select Bluetooth Classic, BLE, Wi-Fi LAN, hotspot,
and optionally WebRTC. See [`NearbyConnectionsManagerImpl`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/nearby_connections_manager_impl.cc#L56-L65)
and its [advertise and discovery setup](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/nearby_connections_manager_impl.cc#L168-L317).

The required automated route matrix is:

| Path               | Discovery or association                                                                   | Data channel and obligations                                                        |
| ------------------ | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| BLE                | fast, GATT, and extended advertisement variants where exposed; scan and GATT in both roles | BLE data plus BLE L2CAP where negotiated; fragmentation, MTU edges, teardown        |
| Bluetooth Classic  | inquiry or service discovery in both roles                                                 | RFCOMM or L2CAP as selected by the reference; refusal, loss, cancellation, teardown |
| Same LAN           | mDNS/DNS-SD advertisement and browse in both roles                                         | TCP in both roles; IPv4 and usable IPv6 candidates, multicast loss and reconnect    |
| Hotspot            | AP create and join in both endpoint roles                                                  | TCP bandwidth upgrade, credential failure, migration, fallback and cleanup          |
| Wi-Fi Direct       | remote group owner and Omarchy group client; connection initiation in both protocol roles  | TCP bandwidth upgrade, migration, fallback and group cleanup                        |
| Upgrade controller | initial low-bandwidth channel                                                              | every permitted transition, rejected upgrade, race, resume and fallback             |

For Wi-Fi LAN, the DNS-SD type is the uppercase hex of the first six SHA-256
bytes of the service ID: `_FC9F5ED42C8A._tcp.` for `NearbySharing`. The instance
and TXT record carry endpoint and endpoint-info fields. See
[`WifiLan::GenerateServiceType`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/mediums/wifi_lan.cc#L497-L508)
and [`WifiLanServiceInfo`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/wifi_lan_service_info.cc#L69-L211).

The Sharing endpoint-info advertisement contains a packed version, name flag
and device type byte, a two-byte salt, a 14-byte encrypted metadata key, an
optional length-prefixed public device name, and optional TLVs. Current TLVs
include vendor and capabilities; the parser also reserves QR. See
[`Advertisement::ToEndpointInfo` and `FromEndpointInfo`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/advertisement.cc#L160-L295).

The Connections enum also names Wi-Fi Aware, NFC, USB, AWDL, WebRTC variants,
and other media. An enum value is not evidence that the current open Sharing
wrapper or Linux product uses it. The public WebRTC class describes itself as a
non-working base implementation. These media are not part of the local Android
baseline without a first-party executable route. See the [medium enum](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/proto/connections_enums.proto#L79-L94)
and [WebRTC base](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/mediums/webrtc.h).

Google announced separate Quick Share paths for [AirDrop interoperability](https://blog.google/products-and-platforms/platforms/android/quick-share-airdrop/)
and [NFC tap-to-share](https://blog.google/products-and-platforms/platforms/android/tap-to-share-android/).
Quick Share also offers QR delivery using a hosted transfer. Those paths depend
on Apple AWDL, OEM NFC integration, or a live Google service and cannot be proved
by the local Nearby harness. They remain explicit compatibility gaps.

## Wire and authentication sequence

The implementation and oracle must agree on this state sequence:

1. Advertise or discover endpoint info using one of the paths above.
2. Open the medium channel. Send a length-delimited `CONNECTION_REQUEST` with
   endpoint fields, collision nonce, supported upgrades, and medium metadata.
3. Run the three-message UKEY2 exchange. The initiator sends `CLIENT_INIT`, the
   responder sends `SERVER_INIT`, and the initiator sends `CLIENT_FINISH`.
   Nearby selects P-256 ECDH with a SHA-512 commitment and next protocol
   `AES_256_CBC-HMAC_SHA256`.
4. Both Connections roles accept or reject. A successful handshake creates a
   directional D2D context with HKDF-derived keys, AES-256-CBC, HMAC-SHA256, and
   strictly increasing sequence numbers.
5. Inside that channel, both Sharing roles exchange `PAIRED_KEY_ENCRYPTION`,
   validate the signature and secret-ID hash where possible, then exchange
   `PAIRED_KEY_RESULT`.
6. The sender sends `INTRODUCTION`. The receiver answers `ACCEPT`, `REJECT`,
   `NOT_ENOUGH_SPACE`, `UNSUPPORTED_ATTACHMENT_TYPE`, or `TIMED_OUT`. Accepted
   referenced payloads follow, with progress, acknowledgement, cancellation,
   keepalive, disconnect, and any bandwidth upgrade.

The three-message roles and timeout are visible in [`EncryptionRunner`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/encryption_runner.cc#L38-L190).
UKEY2's key derivation and message protection are in
[`UKey2Handshake::ToConnectionContext`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/ukey2_handshake.cc#L245-L286),
[`D2DCryptoOps`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/d2d_crypto_ops.cc#L55-L117),
and [`D2DConnectionContextV1`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/d2d_connection_context_v1.cc#L100-L148).

Account-free operation must not skip paired-key messages. If no certificate is
available, Google sends random stand-ins and produces `UNABLE`. An incoming
Everyone-mode session may proceed with user approval and the four-digit token;
a hidden-visibility session rejects that result. Quick Share derives the display
token from the raw 32-byte UKEY2 verification string modulo 9973. See
[`PairedKeyVerificationRunner`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/paired_key_verification_runner.cc#L114-L373)
and [`TokenToFourDigitString`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/share_session.cc#L56-L71).

TCP-like endpoint channels normally use a four-byte big-endian length before a
protobuf, UKEY2 message, or encrypted frame. BLE and BLE L2CAP code has its own
packet and payload-length handling, so a single framing rule must not be applied
blindly to every medium. See [`BaseEndpointChannel`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/base_endpoint_channel.cc#L92-L249)
and [`BleSocket`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/mediums/ble/ble_socket.cc#L47-L94).

One source-level exception needs an explicit test: point-to-point hotspot,
Wi-Fi Direct, and AWDL upgrades can negotiate `supports_disabling_encryption`.
The upgraded channel may then omit D2D wrapping. The Rust state machine must
match both negotiated outcomes instead of assuming that every upgraded byte has
the original wrapper. See [`BwuManager`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/bwu_manager.cc#L716-L720)
and its [outgoing decision](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/bwu_manager.cc#L947-L954).

## Programmatic proof stack

No one tool proves protocol semantics, stock-product behavior, and Linux radio
integration. Each route needs both initiator and responder tests and a
reference-to-reference self-test before it can judge Rust.

| Layer                    | Pinned tool or peer                                                                                                                                                                                                                                                                                                                                         | What it proves                                                                                   | What it does not prove                                                                                                                                                       |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Crypto                   | Google's [`ukey2_shell`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/ukey2_shell.cc) and [`Ukey2CppCompatibilityTest`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/test/java/com/google/security/cryptauth/lib/securegcm/Ukey2CppCompatibilityTest.java) | Live handshake, verification string, both roles, cross-language encryption and sequence behavior | Nearby framing or Sharing state                                                                                                                                              |
| Connections semantics    | A small framed oracle built from pinned Google Nearby, plus [`OfflineSimulationUser`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_simulation_user.h) and core tests                                                                                                                   | Golden frames, state transitions, payload, reject, cancel, disconnect, upgrade decisions         | Linux BlueZ or NetworkManager behavior; the in-process simulator cannot accept a Rust socket itself                                                                          |
| Sharing semantics        | Pinned Google session tests and a versioned oracle wrapper                                                                                                                                                                                                                                                                                                  | Paired-key, introductions, response statuses, attachment mapping, consent and transfer outcomes  | Stock Android UI or medium selection                                                                                                                                         |
| Live Linux peer          | Pinned `kidfromjupiter/nearby`, only after a machine protocol and peer-to-peer self-test are added                                                                                                                                                                                                                                                          | Google-derived end-to-end peer in both Sharing roles                                             | Authority or broad compatibility; its [README](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/README.md) calls it under construction |
| Wi-Fi LAN                | Network namespaces, Avahi or a DNS-SD harness, TCP proxy, `tc netem`                                                                                                                                                                                                                                                                                        | mDNS, candidates, real sockets, delay, loss, reorder and teardown                                | Product semantics without a live peer                                                                                                                                        |
| Bluetooth                | BlueZ virtual controllers, RootCanal or Netsim, and [Bumble](https://google.github.io/bumble/platforms/linux.html)                                                                                                                                                                                                                                          | HCI, advertisements, GATT, L2CAP, RFCOMM, controller and fault behavior                          | A turnkey stock Quick Share peer                                                                                                                                             |
| Hotspot and Wi-Fi Direct | [`mac80211_hwsim`](https://wireless.docs.kernel.org/en/latest/en/users/drivers/mac80211_hwsim.html), wmediumd, hostapd and wpa_supplicant                                                                                                                                                                                                                   | AP/station and GO/client roles, NetworkManager lifecycle, loss, SNR and capture                  | Stock Android route selection                                                                                                                                                |
| Android black box        | Pinned Google Play AVDs, UI Automator and Mobly, only after AVD-to-AVD and AVD-to-Linux self-tests                                                                                                                                                                                                                                                          | Closest no-phone stock-product observation                                                       | A documented headless API, force-medium control, OEM coverage, or long-term image stability                                                                                  |

Android Emulator 36.5 and later documents shared virtual Wi-Fi with NSD and
Wi-Fi Direct between emulator instances. Emulator networking also documents
Bluetooth Classic and BLE from API 31. It does not promise that Quick Share is
present or testable on every image, nor that the Linux host joins that virtual
radio. See the [interconnection guide](https://developer.android.com/studio/run/emulator-networking-interconnect)
and [emulator capability table](https://developer.android.com/studio/run/emulator-networking).
A custom Android `ConnectionsClient` probe can prove generic Nearby Connections,
not the Quick Share Sharing layer, and it cannot prove which internal medium the
stock product selected.

The current experimental Linux fork matches the confirmed local Wi-Fi Direct
scope: its adapter states that group-owner listening is unsupported and
implements the Omarchy group-client role. A future group-owner promise would
need a second implementation and a new executable route. See its
[`wifi_direct.cc`](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/internal/platform/implementation/linux/wifi_direct.cc).

Before application behavior starts, its tools and upstream inputs must be
pinned, its Google-derived state and payload fixtures must pass, and its fast
adapter contract must run against an independently checked transport seam.
Add each live reference route to release verification when deterministic setup,
cleanup, a passing self-test, both applicable roles, negative and fallback
cases, and a child gate under 60 seconds exist. The gate claim is limited to the
paths actually executed. Physical-device coverage remains necessary for OEM
differences, account/contact trust, AWDL, NFC, hosted QR behavior, and stock
medium-selection claims.

## Reproducible oracle builds

The source commit alone is not a reproducible build. Google's current
[`MODULE.bazel`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/MODULE.bazel)
downloads UKEY2, smhasher, Nisaba, and protobuf-matchers from mutable branch
archives without `sha256`. At the research date, the corresponding immutable
source anchors are:

| Dependency        | Commit to pin                                                                                      |
| ----------------- | -------------------------------------------------------------------------------------------------- |
| UKEY2             | `10fc737aa901e873a3367e7e26b88eb01cd55d69`                                                         |
| smhasher          | `07bb4de10a63e8cc2e1724865454eba635742383`                                                         |
| Nisaba            | `fe8f9cb63db9ed91ddd3022835317a75343a594c`                                                         |
| protobuf-matchers | `793247783c7d9e6322c2b40f85ceb775a7f29f49` on `main`; Google's requested `master` no longer exists |

Standalone UKEY2 has the same issue. Its
[`WORKSPACE`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/WORKSPACE)
uses mutable googletest, Abseil, and BoringSSL branches and its build loads
`rules_cc` without declaring it. Audit anchors on the research date are
protobuf `v3.24.4` at `55d8d93777cc9936b0a979d340d387dbcf05388b`,
googletest at `3b49a56fe3958a3a9c3da722f5ee1a3053b6975f`, Abseil at
`cc05267c15127e41d497ddfc985328e59992ea53`, and BoringSSL
`master-with-bazel` at `9c37515289e44989a828c05cda90dd024de6172f`.

The executable strategy is to maintain a repository-owned oracle lock manifest.
For every fetched archive it records the source commit, canonical URL, SHA-256,
license, and applied patch digest. A fetch command must verify every digest and
fail offline or on mismatch. The oracle pins Bazel 9.2.0 and stores a compressed,
hashed lock regenerated for that LTS because Google's checked-in lock format is
not readable by Bazel 9. Repository overrides replace the mutable UKEY2 archive
with its verified source tree. Provisioning builds once with network access and
then repeats the exact targets with `--nofetch` and no container network. The
warm gate runs Google's C++ tests and a live bidirectional UKEY2 session. The
remaining mutable repositories above must receive the same override before a
target that reaches them is accepted. Branch-head commits are audit anchors,
not permission to fetch those branches later.

## Licensing and redistribution

- Google Nearby and UKEY2 are Apache-2.0. A Rust port may use their public source
  if it preserves required copyright, license, and NOTICE material, marks
  modifications, and records provenance. Apache-2.0 includes a patent grant but
  does not grant Google trademarks. See the [Nearby license](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/LICENSE)
  and [UKEY2 license](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/LICENSE).
- The experimental Linux fork, Bumble, RootCanal, Netsim, and Mobly are
  Apache-2.0. They still need individual pin, notice, and provenance records.
- BlueZ programs and Linux virtual-radio tooling include GPL components; Avahi
  is LGPL. Keep these as separate development executables or environments. Do
  not copy or link their code into the distributable binary without a deliberate
  licensing review.
- Android Emulator and Google Play system images are covered by the
  [Android SDK License](https://developer.android.com/studio/terms). Download
  them through the SDK manager after the user accepts the terms. Do not commit,
  mirror, or ship those images in the repository or release bundle.
- The proprietary Windows Quick Share application and Google Play components
  are not covered by the Apache licenses above and expose no supported test API.
- Google branding guidance does not permit integrating a Google trademark into
  a product name or implying endorsement. Review the public plugin name before
  release, use no Google logo or trade dress, and describe interoperability in
  plain text. See Google's [brand guidance](https://about.google/brand-resource-center/guidance/).

This is an engineering baseline, not legal advice. The intended release remains
a small independently built binary with permissively licensed runtime
dependencies. Heavy simulators, SDKs, and reference peers belong only in the
development toolchain and are not prerequisites for users building the binary.

## Corrections to earlier research

This baseline supersedes any earlier statement that:

- the product is receive-only or sends files only;
- Google Nearby supplies working upstream Linux media;
- Google simulation users are external peers that Rust can connect to directly;
- the experimental Linux fork already proves both Wi-Fi Direct roles or provides
  a stable machine oracle;
- every medium uses identical four-byte framing;
- D2D wrapping necessarily remains enabled after every bandwidth upgrade;
- a Google Play AVD automatically proves stock Quick Share compatibility; or
- every Nearby Connections medium enum is part of the local Android Quick Share
  implementation target.
