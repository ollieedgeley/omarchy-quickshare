# Bidirectional programmatic verification

Research date: 2026-09-02

This note extends [programmatic connection testing](../connection-mocking-tools.md) for supported content transfer in both directions:

- inbound: an Android/Google-compatible sender discovers Omarchy, connects, and sends files, plain text, or URLs;
- outbound: Omarchy discovers an Android/Google-compatible receiver, connects, and sends files, plain text, or URLs.

The application direction does not fix the network role. Each direction must also cover the applicable advertiser/browser, listener/client, GATT or L2CAP server/client, and bandwidth-upgrade initiator/acceptor roles.

## Decision

Add four test-only layers before transfer application code starts:

1. Generate language-neutral outbound and inbound session fixtures from pinned Google C++ tests.
2. Run live two-process tests against a pinned Linux build of Google's Nearby code, with a project-owned test control wrapper.
3. Run same-LAN sharing tests against NearShare as an implementation-diverse peer.
4. Orchestrate a pinned Google Play AVD with Mobly, a small Nearby Connections probe APK, and UI Automator. Stock Quick Share cases enter the required suite only after an AVD-to-AVD control and an AVD-to-Linux route pass repeatable self-tests.

Add `tc netem` for IP packet faults and `wmediumd` for 802.11 faults. Keep Toxiproxy for precise TCP stream faults. These tools cover different layers.

Every item above is a development dependency. None is linked into, packaged with, or installed by the Omarchy plugin. Plugin users need the Rust binary and the normal Linux services selected by the runtime design, not Android SDK tools, Python, Mobly, a C++ reference implementation, an emulator, or a test radio.

## What the available peers actually prove

| Tool                                 | Layer                   | Initiator       | Responder       | Suitable claim                                                          |
| ------------------------------------ | ----------------------- | --------------- | --------------- | ----------------------------------------------------------------------- |
| Google `OfflineSimulationUser`       | Nearby Connections      | yes             | yes             | Google Connections behavior inside one simulated process                |
| Linux fork `file_share`              | Nearby Connections      | yes             | yes             | live process and Linux medium interoperability, with media forced       |
| Android `ConnectionsClient` probe    | Nearby Connections      | yes             | yes             | interoperability with closed Google Play services on its selected route |
| Google `outgoing_share_session_test` | Nearby Sharing state    | no live peer    | no live peer    | authoritative local outbound state and wire-object semantics            |
| Linux fork `nearby_sharing_cli`      | Nearby Sharing          | yes             | yes             | live Google-derived Sharing peer                                        |
| NearShare                            | Nearby Sharing over LAN | yes             | yes             | implementation-diverse UKEY2, Sharing frames, and payload transfer      |
| Stock Quick Share on Google Play AVD | complete product        | yes, through UI | yes, through UI | black-box behavior for one pinned virtual Google build, if self-tested  |

Google's [`OfflineSimulationUser`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_simulation_user.h) exposes advertising, discovery, request, accept, reject, payload, cancellation, disconnect, and upgrade calls. Google's own test runs BLE, Bluetooth Classic, and Wi-Fi LAN combinations in both endpoint roles and checks connection, rejection, payload, cancellation, and disconnect behavior. Its bandwidth-upgrade case only checks that the call completes, not that a channel migrated successfully. The simulated media exist inside `MediumEnvironment`, so they cannot connect to a Rust process through an operating-system radio interface. [Source and cases](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_service_controller_test.cc#L66-L126), [upgrade limitation](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_service_controller_test.cc#L535-L544)

The third-party Linux fork at commit `6887b0983200c6c8c29e614ea2633d13bf18315d` adds two useful binaries; they are not a Google-supported Linux product. Its Connections `file_share` accepts `--advertise`, `--discover`, initial media, upgrade media, send paths, and a receive directory; it accepts a real connection and sends file payloads. It can therefore drive forced transport and upgrade tests in either process role, but it stops below Nearby Sharing. [Options](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/connections/file_share/main.cc#L43-L79), [connection and media setup](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/connections/file_share/main.cc#L215-L323), [file payload send](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/connections/file_share/main.cc#L380-L425)

The same fork's `nearby_sharing_cli` supports `receive` and `send FILE`, registers real send or receive surfaces, accepts an incoming share, and calls `SendAttachments` for the first discovered peer. That makes it a true live Sharing initiator and responder, unlike a session unit test. [Commands](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/sharing/linux/nearby_sharing_cli.cc#L70-L124), [send and receive surfaces](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/sharing/linux/nearby_sharing_cli.cc#L286-L340), [accept and send actions](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/sharing/linux/nearby_sharing_cli.cc#L343-L407)

Do not use that CLI unchanged as the sole oracle. It prints human-oriented output, auto-accepts, selects the first peer, does not expose rejection or deterministic mid-transfer cancellation, and does not let Sharing tests force a medium. Its README calls the project buggy and lists pairing, Bluetooth transfer, progress, and advertisement-cleanup problems. [Upstream warning and known defects](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/README.md#L5-L8), [defect list](https://github.com/kidfromjupiter/nearby/blob/6887b0983200c6c8c29e614ea2633d13bf18315d/README.md#L78-L105)

Build a test-only wrapper or maintained test fork with a versioned JSON-lines or Unix-socket control protocol. It must select a peer by stable test ID, report stage and selected medium, choose accept or reject, cancel by payload ID, disconnect at a named stage, force supported initial and upgrade media, and emit a terminal result including byte counts and SHA-256. First run its own upstream suite and a reference-peer-to-reference-peer self-test. A passing wrapper test is not evidence that an upstream defect disappeared.

## Outbound session and payload oracles

Google's [`outgoing_share_session_test.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session_test.cc) is the primary semantic oracle for `outgoing_share_session`. It verifies attachment-to-payload mapping, file metadata and introduction serialization, response mapping for accept, reject, insufficient space, unsupported attachment, and timeout, sequential payload sending, and delayed completion while the receiver disconnects. [Payload construction](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session_test.cc#L181-L262), [introduction](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session_test.cc#L392-L461), [responses and payload ordering](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session_test.cc#L591-L797), [disconnect completion](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session_test.cc#L799-L857)

It is not a responder oracle. The test injects responses and observes sends through [`FakeNearbyConnectionsManager`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/fake_nearby_connections_manager.h#L44-L152); no independent endpoint parses the introduction, accepts it, or receives a file. Google's service-level suite adds successful file sending and sender/receiver cancellation in both connection roles, but it still uses project fakes. [Service-level send and cancellation cases](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/nearby_sharing_service_impl_test.cc#L3386-L3840)

Preserve these tests' value by compiling a pinned Google test helper into a fixture generator. Commit canonical protobuf bytes and event traces with the upstream commit, source path, fixture schema, license, and SHA-256. Run the generator against the committed fixtures to detect upstream drift. Do the same for Google's incoming session scenarios.

Use two live oracles in addition:

- The Linux Google-derived Sharing CLI checks that Rust can exchange real frames and payloads with the same implementation family as the semantic oracle.
- NearShare commit `66eea15c5799ea317b195bedba465fb89ff5da7b` provides implementation diversity for same-LAN transfer. Its UI-free core implements mDNS/TCP discovery, both UKEY2 roles, introduction, accept or decline, and encrypted payloads. Its loopback test transfers two files through the full handshake, compares the PIN, and compares file SHA-256; separate routing tests inject malformed, duplicate, interleaved, and cancelled payloads. [Architecture](https://github.com/Dhiva-Labs/NearShare/blob/66eea15c5799ea317b195bedba465fb89ff5da7b/README.md#L25-L48), [loopback test](https://github.com/Dhiva-Labs/NearShare/blob/66eea15c5799ea317b195bedba465fb89ff5da7b/tests/test_loopback.py#L1-L115), [payload routing tests](https://github.com/Dhiva-Labs/NearShare/blob/66eea15c5799ea317b195bedba465fb89ff5da7b/tests/test_payload_routing.py)

NearShare is not an authority for every medium or trust mode. Its transfer path is same-LAN TCP, outbound discovery requires the phone's Quick Share receive screen or share sheet to be open, and it implements neither contact visibility nor Google-account trust. [Documented limits](https://github.com/Dhiva-Labs/NearShare/blob/66eea15c5799ea317b195bedba465fb89ff5da7b/README.md#L356-L379)

QNearbyShare is a possible extra diversity peer, but not a required gate. Its last source change is older, its automated protocol coverage is small, and it documents that newer Android Quick Share receivers cannot be triggered. Requiring it would add maintenance without closing a distinct proven gap. [Source](https://github.com/vicr123/QNearbyShare/tree/e0917fdf80c866cb61a979a964900b3b3983eb76)

## Android automation

Mobly is useful as an orchestrator, not as a Quick Share API or oracle. It coordinates multiple Android devices and custom controllers and collects `adb` and device artifacts. Mobly Snippet Lib lets a host invoke Java methods in a test APK and includes a UI Automator example. Both projects state that they are Google-developed but not official Google products. [Mobly scope](https://github.com/google/mobly/tree/561991306680bc54061e15a2e32706cf9930f24d), [Snippet Lib mechanism and UI Automator example](https://github.com/google/mobly-snippet-lib/tree/3c705915cad43acd88a8815b3f3e3cf9455a60a4)

Build one small, test-only Android probe with two controls:

- a Nearby Connections service using public `ConnectionsClient` calls for advertise, discover, request, accept, reject, send a `FILE` payload, cancel a payload, disconnect, and stop all endpoints;
- a UI Automator driver for the stock Quick Share UI.

The public API makes Connections symmetric after initiation and reports file transfer success or error. It does not expose an API to force or observe the selected medium or bandwidth upgrade. The probe therefore proves closed-Google Connections compatibility on the route Google chose, not every connection type. [`ConnectionsClient` methods](https://developers.google.com/android/reference/com/google/android/gms/nearby/connection/ConnectionsClient), [connection acceptance](https://developers.google.com/nearby/connections/android/manage-connections), [file payload completion](https://developers.google.com/nearby/connections/android/exchange-data)

There is no documented headless Quick Share test API. UI Automator can drive visible elements across application and system UI under `AndroidJUnitRunner`, so it can automate the available black-box route. [UI Automator scope](https://developer.android.com/training/testing/other-components/ui-automator)

Use this flow on a pinned English-locale Google Play AVD:

1. Self-test stock Quick Share AVD-to-AVD for both send and receive before involving Rust.
2. For Android to Omarchy, have the probe issue `ACTION_SEND`, select Quick Share and the named Omarchy peer, then accept the PIN on the Omarchy control seam.
3. For Omarchy to Android, open the Quick Share receive or visibility screen, wait for the named sender, and select accept or reject in the dialog or notification.
4. Verify the received file through a probe-owned `ContentResolver` read or a permitted `adb` pull and compare SHA-256.
5. Save screenshots, the UI hierarchy, logcat, selected-medium telemetry when available, and Bluetooth HCI snoop or packet captures on failure.

Pin the emulator binary, system-image package ID and digest, API level, Google Play services version, locale, display size, snapshots, Mobly, Snippet Lib, AndroidX Test, and probe APK digest. Resource IDs and text belong to closed, updateable Google UI, so each image update must pass the AVD-to-AVD control before the Rust cases run.

Android Emulator 36.5 and later places emulator instances on shared virtual Wi-Fi with Network Service Discovery and Wi-Fi Direct; its capability table lists Bluetooth Classic and BLE from API 31. That documentation promises emulator-to-emulator connectivity, not a Linux host as a peer on the same virtual media. [Multi-emulator network](https://developer.android.com/studio/run/emulator-networking-interconnect), [capability table](https://developer.android.com/studio/run/emulator-networking)

Bumble can attach a host stack to the emulator's Netsim Bluetooth network, where it can communicate with the Android Bluetooth stack and apps. Its guide warns that the emulator integration is evolving and that custom-controller attachment may not be officially supported. Treat Android-to-Linux Bluetooth as provisional until a pinned self-test crosses Netsim, the Linux Bluetooth stack, and a reference endpoint. [Bumble Android integration](https://google.github.io/bumble/platforms/android.html#connecting-to-netsim)

Cuttlefish remains useful for controllable AOSP radio tests. It exposes RootCanal controls for Bluetooth and `wmediumd_control` for Wi-Fi SNR, position, and packet capture. A public AOSP image does not include Google apps or services, while an AVD system image labelled with Google APIs includes Play services. Stock Quick Share on Cuttlefish therefore requires a separately licensed image and must not be assumed or redistributed. [Cuttlefish connectivity controls](https://source.android.com/docs/devices/cuttlefish/connectivity), [AVD image distinction](https://developer.android.com/studio/run/managing-avds)

## Fault tools are complementary

Toxiproxy is a TCP stream proxy. It can apply directional latency, bandwidth, timeout, reset, slicing, and exact byte limits after a socket has been redirected through it. It cannot damage UDP multicast discovery or model Wi-Fi association and RF conditions. [Toxiproxy design and toxics](https://github.com/Shopify/toxiproxy/tree/40f7fd31bee529d824116bd2a11a9e3425e904ec)

`tc netem` operates on a Linux interface queue and can add seeded delay, loss, corruption, duplication, reordering, and rate limits. Place each peer in its own network namespace and attach it to the test interface. This covers UDP and multicast mDNS as well as TCP, subject to the selected ingress and egress arrangement. It still does not model association or radio propagation. [netem manual](https://man7.org/linux/man-pages/man8/tc-netem.8.html)

`wmediumd` sits below IP on `mac80211_hwsim`. It adds per-link 802.11 frame loss and delay and can use probability or path-loss models, including asymmetric links. Separate network namespaces are required so local traffic actually crosses the simulated medium. This adds discovery, association, group formation, and radio-collapse coverage that neither Toxiproxy nor netem provides. [Upstream design and namespace warning](https://github.com/bcopeland/wmediumd/tree/717e5d7fcc23eecbc8e32bd897a8fd4b1e3ba640)

Every injector needs a self-test that first proves an unmodified control flow, then proves the configured fault is visible in a packet capture or endpoint result, and finally proves cleanup restores the control flow. Never infer that a fault ran from the injector command's exit status.

## Bidirectional medium and upgrade matrix

Run each supported row with the Google-derived Connections peer in both application directions. For each direction, swap the connection initiator and responder where the protocol permits. A Sharing-level case must also run where the reference Sharing peer supports that medium.

| Route                                   | Roles to exercise                                                           | Success evidence                                 | Required injected failure                                   |
| --------------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------ | ----------------------------------------------------------- |
| BLE advertisement, GATT, BLE L2CAP/data | advertiser/scanner; server/client                                           | discovery, authenticated channel, exact file SHA | lost advert, GATT reject, channel close, controller removal |
| Bluetooth Classic                       | discoverable/discoverer; SDP and RFCOMM or L2CAP server/client              | selected medium, exact file SHA                  | pairing reject, socket reset, daemon/controller loss        |
| Same LAN                                | mDNS advertiser/browser; TCP listener/client                                | service found, exact file SHA                    | dropped mDNS, delayed/reordered IP, half-close, reset       |
| Wi-Fi hotspot                           | group owner/joiner; TCP listener/client                                     | association, address, exact file SHA             | association timeout, owner loss, profile cleanup            |
| Wi-Fi Direct                            | remote group owner and Omarchy group client; connection initiator/responder | group formed, address, exact file SHA            | negotiation timeout, group loss, cleanup                    |

The current product seam promises Wi-Fi Direct group-client join, not Omarchy group-owner hosting. Do not claim the unpromised role. If hosting is later added, add the inverse group-owner row before implementation.

For every supported initial-to-upgrade pair, force and observe both endpoints as upgrade proposer and upgrade acceptor. At minimum cover BLE to Bluetooth Classic, BLE or Classic to LAN, hotspot, and Wi-Fi Direct where the pinned implementation advertises that pair. Record the old and new medium, cutover sequence, and one final SHA. Test success, rejection, candidate disappearance before cutover, new-channel loss after establishment, simultaneous proposals, old-channel fallback, and cancellation during migration. Assert one committed file, monotonic progress, no duplicate bytes, and cleanup of both channels.

Google's internal bandwidth-upgrade handler tests are useful semantic references for Bluetooth, Wi-Fi LAN, hotspot, and Wi-Fi Direct, but they do not constitute Rust interoperability. Live forced-medium tests must cross the Linux operating-system interfaces or approved virtual radios. [Bandwidth-upgrade test sources](https://github.com/google/nearby/tree/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation)

## Decisions and failure injection

Every full Sharing peer must support these deterministic commands or fixtures:

- accept, reject, insufficient space, unsupported type, and no-response timeout;
- sender cancel before consent, sender cancel during payload, receiver cancel during payload, and disconnect at each state boundary;
- correct PIN and keys, mismatched PIN, bad MAC, replayed frame, illegal sequence, and handshake timeout;
- zero-byte, tiny, chunk-boundary, multi-chunk, large, and multiple-file payloads;
- truncated, overrun, duplicate, out-of-order, unknown-ID, and interleaved payload frames;
- disk full, permission failure, unsafe path, symlink traversal, duplicate name, and final rename failure;
- advertisement loss, mDNS loss, service restart, controller removal, association loss, TCP half-close or reset, SNR collapse, and upgrade failure.

After every terminal result, assert that advertisements, GATT applications, Bluetooth profiles, sockets, temporary files, network profiles, hotspot or Direct groups, namespaces, child processes, and test credentials are gone. Run the next clean transfer in the same environment to catch leaked state.

## Pre-application verification gate

The environment is ready for application TDD only when all applicable rows below are green from a clean machine. These become child gates under the root `Makefile` when setup is authorized. Each directly runnable test phase must complete in less than 60 seconds; split by medium or virtual environment when necessary. Prepared-environment startup and teardown follow the separate lifecycle budget in the programmatic connection-testing policy.

| Gate                  | Self-test before Rust exists                                                           | Required evidence                                                 |
| --------------------- | -------------------------------------------------------------------------------------- | ----------------------------------------------------------------- |
| Oracle fixtures       | pinned Google generator against committed inbound and outbound fixtures                | byte equality, event-trace equality, source commit and hashes     |
| Sharing reference     | Google-derived Sharing peer to itself, send and receive                                | accept and reject, cancel, exact file SHA, clean second run       |
| Diverse LAN reference | NearShare to itself and NearShare to Google-derived peer both ways                     | mDNS discovery, matching PIN, exact file SHA, cleanup             |
| Connections reference | two `file_share` peers for every forced initial medium and upgrade pair, roles swapped | observed media, exact file SHA, clean fallback                    |
| Bluetooth lab         | BlueZ/Bumble or RootCanal reference endpoints through real Linux interfaces            | BLE and Classic role matrix, captures, controller cleanup         |
| Wi-Fi lab             | two reference endpoints over hwsim/wmediumd and namespaces                             | LAN, hotspot, Direct rows, captures, group cleanup                |
| Fault injectors       | reference peer control, injected fault, restored control                               | Toxiproxy TCP, netem UDP/IP, wmediumd 802.11 fault observed       |
| Android Connections   | two probe APKs, then probe against Linux reference in both roles                       | accept, reject, FILE send, cancel, SHA, selected route if exposed |
| Stock Quick Share     | AVD-to-AVD control, then AVD-to-Linux reference in both directions                     | UI trace, accept/reject/cancel, exact file SHA, clean second run  |

If the stock Quick Share AVD cannot discover a Linux peer through a documented bridge, mark that route unsupported with the captured failure. Do not weaken the self-test or count the public Nearby Connections API as a Sharing test. The rest of the deterministic verification remains required; the exact stock Quick Share gap stays explicit and physical-phone evidence remains manual and non-gating.

## What simulation can and cannot prove

The proposed suite can reliably prove:

- inbound and outbound state semantics against pinned Google fixtures;
- live Rust interoperability with the pinned Google-derived Linux peer on every self-tested virtual Linux transport;
- same-LAN Sharing interoperability with an implementation-diverse peer;
- both Nearby Connections roles against closed Google Play services on the route it selects;
- stock Quick Share behavior for one pinned Google Play AVD only after both control and cross-host routes pass;
- deterministic consent, rejection, cancellation, timeout, corruption, upgrade, fallback, and cleanup behavior covered by the matrix.

It cannot prove without physical devices:

- compatibility with every Android release, Google Play services build, Samsung or other vendor variation, chipset, driver, or firmware;
- real RF scheduling, antenna, coexistence, roaming, and range behavior;
- OEM background restrictions, doze, lock-screen, battery, and permission presentation;
- contact, account, and "Your devices" certificate trust that depends on live Google services and accounts;
- every medium selected by stock Quick Share, because its public UI and Nearby Connections API neither force nor report every internal route;
- stock Quick Share on an ordinary public Cuttlefish AOSP image.

Physical-device checks remain compatibility evidence, never a local hook or automated-gate dependency.

## Licensing and distribution

Pin every tool by repository, commit or package version, source path, license, and artifact hash in the future test-tool manifest.

- Google Nearby, the Linux Nearby fork, Bumble, Mobly, and Mobly Snippet Lib are Apache-2.0. Preserve notices for any vendored or modified source. [Nearby license](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/LICENSE), [Mobly license](https://github.com/google/mobly/blob/561991306680bc54061e15a2e32706cf9930f24d/LICENSE)
- NearShare and Toxiproxy are MIT. Preserve their license text if source or binaries are cached or redistributed within a developer bundle. [NearShare license](https://github.com/Dhiva-Labs/NearShare/blob/66eea15c5799ea317b195bedba465fb89ff5da7b/LICENSE), [Toxiproxy license](https://github.com/Shopify/toxiproxy/blob/40f7fd31bee529d824116bd2a11a9e3425e904ec/LICENSE)
- wmediumd and iproute2 are GPL-2.0 tools. Execute host-installed or isolated test binaries; do not link their code into the permissive Rust product. [wmediumd license](https://github.com/bcopeland/wmediumd/blob/717e5d7fcc23eecbc8e32bd897a8fd4b1e3ba640/LICENSE), [iproute2 source license](https://kernel.googlesource.com/pub/scm/network/iproute2/iproute2/+/df210e83e0fab40209a71c70cd089fc1d66e275e/Makefile)
- Android SDK and Google system-image artifacts are governed by the Android SDK License. It restricts copying, modification, and redistribution except where an open-source component license applies. Developers must install them through the approved SDK tooling and accept the terms; the repository must not mirror Google Play images or closed SDK components. [Android SDK License sections 3.1 to 3.5](https://developer.android.com/studio/terms#3-sdk-license-from-google)

These licenses do not change the runtime promise: the published plugin must not depend on or install any test-only peer, Android tool, Python package, C++ library, emulator image, Toxiproxy, netem, or wmediumd.
