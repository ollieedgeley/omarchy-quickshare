# Lean Rust Quick Share endpoint feasibility

Research date: 2026-09-02

## Verdict

Yes. We can build a permissively licensed, account-free Rust endpoint from Google's Apache-2.0 sources. It can receive supported attachments and send files without shipping the C++ stack, Bazel, Qt, Java, ADB, or the Android SDK to users.

It should not be a line-by-line conversion of `google/nearby`. The useful design is a focused Rust implementation of the Android-to-Linux and Linux-to-Android paths, with Google's C++ tests and the Linux port acting as executable references. Google's non-test Connections and Sharing implementation is about 66,000 lines of C++ before Linux platform code. Much of that supports generic Nearby topologies, accounts, contacts, sync, analytics, and platform abstractions that this daemon does not need. The [`sharing/BUILD`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/BUILD) and [`connections/implementation/BUILD`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/BUILD) files make that split visible.

My recommendation is to proceed, but call the first release an experimental endpoint rather than "Quick Share for Linux." The earlier 18 to 29 engineer-week estimate covered only the receive path. It is a lower bound, not a schedule for this bidirectional project. Re-estimate after the outgoing oracle proves peer discovery, initiator handshakes, consent, payload sending, cancellation, and cleanup.

## What must be implemented

There are two protocols layered on the same connection. Nearby Connections discovers a peer, authenticates it, carries encrypted frames, moves payloads, and upgrades the transport. Nearby Sharing adds device advertisements, PIN or certificate verification, attachment introductions, consent, and transfer completion. Google's [Connections README](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/README.md) describes the medium-independent connection and upgrade model. Its platform table also shows why a TCP-only clone cannot claim broad coverage.

A bidirectional engine needs these modules:

| Module                      | Required behavior                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Advertisement and discovery | Encode public `Everyone` endpoint information for inbound discovery, decode peer advertisements for outbound discovery, rotate endpoint IDs where required, and understand current v1/v2 capability fields. Google's format is implemented in [`advertisement.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/advertisement.cc).                                                                                                                                                                                                                                               |
| Initial connection          | BLE fast and extended advertisements, connectable and initiating GATT paths, Bluetooth Classic listening and discovery, and Wi-Fi LAN DNS-SD plus TCP in both local roles. BLE cannot be treated only as a wake-up beacon because newer flows can begin over a BLE socket.                                                                                                                                                                                                                                                                                                                                                 |
| Secure channel              | Nearby connection request and response frames, UKEY2 initiator and responder roles, encrypted D2D messages, authentication token generation, sequence checking, timeouts, keepalives, and clean disconnects.                                                                                                                                                                                                                                                                                                                                                                                                               |
| Payload transfer            | Send and receive FILE and BYTES payload headers and chunks. Enforce ordering, acknowledgements, cancellation, progress, bounded allocation, safe source reads, temporary destination files, and atomic completion.                                                                                                                                                                                                                                                                                                                                                                                                         |
| Bandwidth upgrades          | Negotiate and migrate an active encrypted connection to same-LAN TCP, Wi-Fi hotspot, or Wi-Fi Direct without losing or duplicating frames. Fall back to Bluetooth or BLE if an upgrade fails. [`offline_wire_formats.proto`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/proto/offline_wire_formats.proto) contains the upgrade messages and credentials.                                                                                                                                                                                                    |
| Sharing sessions            | For inbound shares, parse introductions, validate limits, expose the PIN, accept or reject, and map payloads to attachments. For outbound shares, build file, plain-text, and URL introductions, expose progress, wait for peer consent, send payloads, and report the peer's terminal result. Google's flows live in [`incoming_share_session.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/incoming_share_session.cc) and [`outgoing_share_session.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session.cc). |

For broad Android coverage, implement all local media and roles that the Sharing layer negotiates: BLE, Bluetooth Classic, Wi-Fi LAN, hotspot, and Wi-Fi Direct. Preserve transport negotiation even when a medium or role is unavailable on a particular adapter. That lets the peers choose another common path.

WebRTC can remain disabled. The public Google tree exposes only a [non-working WebRTC base](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/mediums/webrtc.h), so there is no first-party interoperable implementation to port. Wi-Fi Aware, NFC, USB, and Apple AWDL are generic Nearby media, but the open desktop Sharing implementation does not depend on them. Omitting them does not remove a usable path present in the open Linux reference.

## What can be left out

The lean scope is account-free public discovery with manual PIN confirmation. Inbound sharing uses `Everyone` visibility. Outbound file sharing discovers peers that make themselves publicly available. Google's paired-key exchange must still be spoken, but an account-free endpoint can report `UNABLE` for unavailable certificate verification and use the authentication token for user confirmation. Google's [`paired_key_verification_runner.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/paired_key_verification_runner.cc) explicitly treats `UNABLE` as acceptable for an incoming connection advertised to everyone.

This removes a large, separable product subsystem:

- Google sign-in, OAuth, identity RPCs, contact downloads, public and private certificate storage, encrypted metadata keys, `Contacts`, and `Your devices`
- QR-code send flows, app-bundle sending, streams, and Wi-Fi credential sharing
- P2P cluster and star strategies, because Quick Share uses point-to-point
- cross-device file sync and binding, background contact scheduling, analytics, experimentation infrastructure, update services, and the Google UI
- generic platform interfaces that only exist to support several operating systems

This is not speculative. The Linux port returns no current account and reports sign-in and access tokens as unsupported in [`linux_account_manager.cc`](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/sharing/linux/platform/linux_account_manager.cc). Google's certificate target has direct account, RPC, identity, and scheduler dependencies in [`sharing/certificates/BUILD`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/certificates/BUILD). Public discovery and PIN confirmation avoid that subsystem in both directions.

Accept file, text, URL, and Android app introductions from the first public alpha. Save app payloads as ordinary files. Outbound sharing sends files, plain text, and URLs. Wi-Fi credential and stream attachments can be parsed and rejected cleanly at first, then added if field reports show they matter. Attachment support does not change whether peers can discover and connect.

## Rust implementation choices

Google publishes the wire definitions but no Rust Nearby engine. Its Sharing proto build already contains a [`rust_proto_library`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/proto/BUILD), although it does not generate the complete transfer stack. A registry search on the research date found no maintained UKEY2 or Quick Share crate. UKEY2 and D2D therefore need a small audited Rust port.

The cryptography is available as well-maintained components:

- [`p256`](https://github.com/RustCrypto/elliptic-curves/tree/master/p256) for ephemeral P-256 ECDH
- RustCrypto [`sha2`, `hkdf`, and `hmac`](https://github.com/RustCrypto) for commitments, key derivation, and authentication
- RustCrypto [`aes`](https://github.com/RustCrypto/block-ciphers/tree/master/aes) and [`cbc`](https://github.com/RustCrypto/block-modes/tree/master/cbc) for the D2D secure-message construction
- [`prost`](https://github.com/tokio-rs/prost) for generated proto2/proto3 messages

Do not invent a new secure channel. Port the byte-exact UKEY2 and D2D behavior and import Google's test vectors. The [UKEY2 protocol description](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/README.md) specifies framing, P-256 with SHA-512, commitments, transcript-based HKDF, and authentication strings. Its [`d2d_crypto_ops.cc`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/d2d_crypto_ops.cc) and [`d2d_connection_context_v1.cc`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/d2d_connection_context_v1.cc) define AES-256-CBC, HMAC-SHA256, directional keys, and sequence handling. RustCrypto's P-256 documentation warns that the crate has not received an independent audit, so the complete construction still needs a cryptography review even though its primitives are established.

Use a small [`tokio`](https://github.com/tokio-rs/tokio) feature set for sockets, timers, signals, and one runtime. Avoid its `full` feature. Use [`zbus`](https://github.com/dbus2/zbus) directly for BlueZ, NetworkManager, and optionally Avahi. It has no C library dependency. [`BlueR`](https://github.com/bluez/bluer) is the official BlueZ Rust interface and is useful as a source or for its L2CAP and RFCOMM socket support, but its `bluetoothd` feature currently requires libdbus. Direct D-Bus bindings keep the executable boundary smaller and give control over the BlueZ profile and GATT behavior that Nearby needs.

A detailed [project structure](architecture/project-structure.md) now fixes the workspace shape. It separates generated wire code from audited cryptography, separates Connections from Sharing, gives BlueZ, networking, and safe storage their own adapter crates, and keeps the final daemon and CLI in one binary package. That finer split prevents media code, test infrastructure, and generated sources from growing into the flat directories visible in Google's implementation.

Generate Rust protobuf code through a pinned local build target and commit or package the generated sources. End users then need neither `protoc` nor Bazel.

## Linux dependencies and footprint

One Rust executable can contain the protocol, protobuf, crypto, async runtime, and mDNS client. It will still use operating-system services over D-Bus:

- BlueZ for BLE advertisements, GATT, L2CAP, and Bluetooth Classic profiles
- NetworkManager for hotspot and Wi-Fi Direct lifecycle
- systemd user services and the system D-Bus broker
- Avahi only if we choose its D-Bus API instead of an in-process mDNS implementation

Those are Linux hardware and network services, not Android developer tools. The current Linux port lists `systemd`, NetworkManager, and BlueZ 5.85 or newer as prerequisites in its [README](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/README.md). Its Linux platform target also links sdbus-c++, libcrypto, libcurl, and Bluetooth libraries in [`internal/platform/implementation/linux/BUILD`](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/internal/platform/implementation/linux/BUILD). The Rust design can remove those dynamic library requirements by using zbus and RustCrypto.

Do not set a binary-size promise before the radio prototype. A stripped release with thin LTO, abort-on-panic, and limited crate features should plausibly land in the single-digit to low-teens megabytes. The current Qt AppImage is [43.8 MB](https://github.com/kidfromjupiter/nearby/releases/download/v0.6/QuickShare-x86_64.AppImage), but most of that comparison is UI and C++ packaging rather than protocol logic. Idle memory and Bluetooth activity matter more than the file size. Inbound visibility defaults to off or a timed window, while outbound discovery runs only after a user action.

## Compatibility and maintenance risk

Rust does not create device compatibility by itself. Compatibility comes from copying Google's state transitions, capability negotiation, retry rules, timeouts, medium ordering, and quirks. A smaller binary that simplifies those behaviors too aggressively will work on the developer's phone and fail elsewhere.

The Linux port shows both feasibility and the cost. It added roughly 22,500 lines across 225 Linux-related files, and at the research snapshot it was 39 upstream commits behind Google. Its README calls the implementation buggy, warns that Bluetooth advertisements are not cleaned up, notes interference with other Bluetooth devices, and reports very slow Bluetooth Classic transfers. Its Wi-Fi Direct implementation supports the group-client role and explicitly rejects autonomous group-owner listening in [`wifi_direct.cc`](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/internal/platform/implementation/linux/wifi_direct.cc). These are useful boundaries for the Rust design, not details to hide.

Physical phones are outside the automated verification system. Without a device lab, the release process substitutes repeatable protocol evidence and records community hardware reports separately:

- run Google's Connections, Sharing, and UKEY2 vectors against the Rust modules
- create Rust-to-C++ loopback tests for every initial medium and upgrade with Rust as initiator and responder, in both success and fallback cases
- fuzz every plaintext and encrypted frame boundary with strict size and state limits
- test BlueZ and NetworkManager D-Bus behavior with recorded method and signal fixtures
- ship transport-stage diagnostics with peer names and filenames removed
- maintain a public matrix by phone model, Android version, Quick Share version, Wi-Fi and Bluetooth chipset, and failed stage
- keep protocol constants in one directory with the exact Google source commit recorded beside each ported file

The absence of physical phones is still a release risk. Loopback tests can prove byte compatibility and state-machine behavior, but they cannot prove that Samsung firmware, a particular Wi-Fi driver, or a new Google Play Services build will advertise the same way. The plugin should update independently of Omarchy so protocol fixes can ship quickly.

## Licensing and provenance

Google Nearby, Google UKEY2, and the Linux fork are Apache-2.0. The proposed RustCrypto, prost, Tokio, zbus, and BlueR dependencies use Apache-2.0, MIT, or a choice of the two. A Rust port can therefore stay Apache-2.0, provided it preserves required copyright and notice text and documents modifications. See the [Nearby license](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/LICENSE) and [UKEY2 license](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/LICENSE).

Keep the implementation's provenance limited to those permissive sources. Do not copy code from GPL Quick Share clients into this core. Independent clients may be useful for black-box interoperability tests, but mixing their implementation into an Apache-2.0 daemon would complicate distribution. Branding and the "Quick Share" name need a separate trademark review before publication; the code licenses do not grant Google's product branding.

## Implementation sequence

| Phase                | Deliverable                                                                                                                                                                                |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Protocol oracle      | Pin Google sources, generate all protos, exercise UKEY2 and D2D in both roles, import vectors, and create the C++ differential harness                                                     |
| LAN transfer         | Advertisement and discovery, TCP endpoint, initiator and responder Connections state machines, bidirectional files, plain text, and URLs, PIN, consent, safe writes, and safe source reads |
| Radio entry paths    | BLE advertising and discovery, both GATT roles, BLE sockets, Bluetooth Classic roles, reconnect, and cleanup behavior                                                                      |
| High-bandwidth paths | Wi-Fi LAN migration, hotspot and Wi-Fi Direct role negotiation, transfer in both directions, and fallback                                                                                  |
| Release hardening    | Limits, fuzzing, suspend and adapter-loss handling, diagnostics, systemd packaging, plugin control API, and community beta                                                                 |

Estimate these phases only after the bidirectional oracle and simulator self-tests pass. The receive-only estimate above is a lower bound and must not be used for planning.

The go/no-go gate should come after the radio entry phase. Continue only if the Rust endpoint interoperates with the C++ reference as initiator and responder over LAN, BLE/GATT, and Bluetooth Classic, then upgrades cleanly to TCP in each direction. If that passes, finishing the Rust engine is a sound long-term choice. If it does not, shipping the C++ Linux port behind the same daemon API is safer than weakening the compatibility promise.
