# LocalSend PINless modes and network context

Status: needs-triage
Research date: 2026-09-06. Research only. No automatic acceptance implementation is authorized.

## Recommendation

Use LocalSend as comparative evidence within the independent device-trust epic, not as another protocol to implement in this UX round. Its protocol requires LocalSend on the peer; its convenience settings are not configuration switches for stock Android Quick Share. Do not describe every PINless mode as equally secure.

Current inspected LocalSend source has real certificate-based checks in its normal HTTPS app-to-app path. That is stronger than matching a discovery name, IP, or preferred-device identifier. But its initial favorite enrollment is not an out-of-band ownership verification, HTTP removes those checks, and browser compatibility creates a certificate-less fallback that needs investigation before recommending unattended favorite acceptance.

Option C, LAN membership plus an approved network plus Bluetooth presence, is useful as a restriction on when an already authenticated peer may act. It is not a replacement for peer authentication. If offered without verified identity, label it an explicitly less-secure convenience mode, default it off, and state that another reachable device may send unwanted content. Never call it multi-factor identity verification.

## Evidence scope

- Package database observation: `pacman -Q localsend-bin localsend` reported `localsend 1.18.2-1`; `localsend-bin` was absent. No application was launched and no local settings, keys, account, clipboard, or network state was inspected.
- Primary source baseline is LocalSend commit `6279d3e30d1d1290caee3b81549f8128a8b01d9f`. Links below are immutable. Additional `v1.18.2` tag reads covered persistence, the favorite model, server provider, and security helper; they do not establish that the installed package contains every current-commit check.
- Protocol baseline is `62bd3406ec80d62f2ed46269cdc06c4dcc391083`, whose README describes the v2 upload/download protocol. The application HTTP provider explicitly constructs v2 clients. This is not an audit of every v3/WebRTC path or older mobile release. [Protocol][protocol] [HTTP provider][http]
- The official website, read on the research date, says HTTPS encrypts transfers and optional PIN verification adds security. The README says certificates are generated on each device. These are official product claims, not proof of authenticated enrollment, safe HTTP operation, or equal protection across modes. The source qualifications below matter. [Website][website] [README][readme]
- Source inspection only. No exploit, TLS handshake, transfer, build, test suite, discovery scan, or other runtime validation ran. Existing local project trust research supplies the Quick Share comparison; no application code was changed. [Prior audit](../trusted-devices.md)

## Settings and direction

These defaults describe the inspected source, not the user's saved preferences or all historical releases. [Persistence][persistence] [Settings mapping][settings] [Modes][modes]

| Control                      | Default    | Exact effect and limit                                                                                                                                                                                                                                      |
| ---------------------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Quick save                   | `paired`   | Automatically accepts non-message file offers from favorites. `on` accepts non-message file offers from any sender that passes earlier protocol gates; `off` disables this quick-save path. It is receiver authorization, not sender identity verification. |
| Receive PIN                  | Unset      | Optional saved string checked on incoming `prepare-upload`. Setting it does not turn off quick save; PIN succeeds first, then acceptance policy runs.                                                                                                       |
| HTTPS                        | `true`     | Enables TLS and certificate checks. HTTP remains representable and cannot provide certificate-backed favorite identity.                                                                                                                                     |
| Favorites                    | Empty list | Persist fingerprint, IP, port, alias, local record ID, and custom-alias flag. A favorite is not account ownership.                                                                                                                                          |
| Share via link auto-accept   | `false`    | The device hosting files automatically approves browser clients requesting to download them. This authorizes outbound disclosure, not inbound saving.                                                                                                       |
| Receive via link auto-accept | `false`    | While browser upload mode is served, automatically accepts non-message incoming file requests. It does not authenticate the uploading browser as an enrolled device.                                                                                        |
| Auto finish                  | `false`    | Separate completion UX setting, not permission to receive and not a PIN bypass.                                                                                                                                                                             |

The storage-v3 migration preserves old quick-save `true` as `on`; otherwise it chooses the new `paired` default and removes `ls_quick_save_from_favorites`. It also resets HTTPS to true. Thus an upgraded configuration that previously had global quick save off is not evidence that all future automatic acceptance is off. [Migration][migration]

`ReceiveController.onPrepareUpload` excludes message sessions from quick save, including the favorite and receive-via-link branches. For eligible file offers it calls `acceptFileRequest` with every offered file. This is not an unconditional silent acceptance of arbitrary text. The send-via-link controller separately applies download auto-accept after the server's PIN gate. [Receive controller][receive] [Send controller][send-controller]

LocalSend's receive PIN is a configured request secret, sent as `?pin=...`. It is not Quick Share's transcript-derived, session-specific UKEY2 comparison code. The server checks it before parsing the upload introduction and consulting consent. Three wrong submitted PINs block that IP with HTTP 429; a missing PIN returns 401 without incrementing the counter, and success clears that IP's counter. The counter is an in-memory, 200-entry LRU, not a durable account lockout or protection against many source addresses. [PIN check][pin] [Server state][server] [Upload handler][server-v2]

Consequently, "no PIN configured" and "no receiving prompt" are independent. HTTPS plus manual consent can be PINless; HTTPS plus paired quick save can be PINless for matching favorites; global quick save without a PIN deliberately accepts a much wider sender population.

## What the certificate actually proves

### Persistence and certificate validity

The app generates and saves `ls_security_context` only when absent, then reloads it. It stores private key, public key, certificate, and certificate hash through preferences. A reset action generates and persists a replacement. This is a durable local identity, not a fresh certificate for each transfer. The inspected path does not establish hardware-backed or encrypted-at-rest key storage. [Persistence][persistence] [Security provider][security]

The current Rust generator makes an RSA-2048 key and self-signed certificate, with SHA-256 over DER as the uppercase fingerprint. Its documented default validity is 1975 to 4096. Verification checks time validity and the self-signature, and can check an expected public key when supplied. This is not a CA identity check. Certificate common name `LocalSend User` carries no ownership meaning. Older stored certificates can have different validity periods. [Certificate implementation][cert]

### Sending to an HTTPS app receiver

`SendService` creates `httpProvider.pinnedTo(target.fingerprint)` before preparation. The client's `PinnedServerCertVerifier` checks the certificate's fingerprint during the TLS handshake, before request metadata or payload can be sent. It checks the certificate self-signature/time and delegates TLS handshake signatures to rustls's verifier, proving possession of the corresponding private key. Hostname/SAN matching is deliberately not used because peers are addressed by IP. [Send provider][send] [HTTP provider][http] [Server-certificate verifier][server-cert] [Client construction][client]

Discovery is different. An unpinned discovery client accepts any valid self-signed certificate, then learns the actual TLS certificate fingerprint. HTTPS discovery uses that value instead of the JSON claim; multicast responses can pin the advertised fingerprint while connecting. A successful first connection proves continuity with that newly learned key, not that its owner is the user's intended phone. An attacker present at first contact can supply its own key and familiar alias. [Discovery][discovery]

The normal send provider explicitly pins the selected peer. The existence of a lower-level `try_new_without_cert` client that accepts invalid certificates, and post-response verification helpers, is not evidence that the normal app send path uses those weaker checks. Conversely, a fingerprint supplied to a TLS verifier cannot protect a request made over HTTP. [V2 client][client-v2]

### Receiving from an HTTPS app sender

The normal server requires a client certificate when web sharing is disabled. It validates the self-signed certificate and TLS handshake signatures; it accepts unknown valid certificates at the transport layer so the app can decide consent. `prepare-upload` exposes the actual TLS certificate fingerprint to the app, and the receive controller uses it for favorite matching rather than the self-reported JSON value. HTTPS registration also suppresses discovery registration events when the claimed fingerprint does not match the client certificate. [Client-certificate verifier][client-cert] [Server][server] [Upload handler][server-v2] [Receive controller][receive]

Thus, in the normal certificate-required HTTPS path, favorite auto-accept is cryptographically tied to possession of the saved certificate's key. It is not merely a matching name or source IP. This claim depends on how the favorite was enrolled and on retaining the certificate-required transport boundary.

Important exception: web sharing makes client certificates optional for browser compatibility. `ReceiveController` uses `event.certFingerprint ?? event.info.fingerprint`; the paired quick-save branch has no additional certificate-present requirement. The v2 upload router remains available when v2 is enabled, and its preparation handler has no equivalent missing-certificate rejection. [Server][server] [Upload handler][server-v2] [Receive controller][receive]

The complete inspected route retains v2 in the Rust app bridge for every web mode. `prepare-upload` checks the upload PIN, nonempty files, and the free session slot before emitting the event. Both bridge layers preserve a null certificate fingerprint, and the app dispatches the event directly to the receive controller. Browser upload mode supplies its web PIN instead of the normal receive PIN; browser download mode keeps the receive PIN for this upload route. The download PIN protects the separate download route. No separate `isWeb` or certificate-present guard was found before paired quick save. [Rust bridge][rust-bridge] [Isolate bridge][isolate-bridge] [Server provider][server-provider] [Upload handler][server-v2]

[INFERENCE] With web sharing active, paired quick save, a matching favorite, an eligible non-message file offer, a free session slot, and an unset or satisfied upload PIN, the inspected source permits a certificate-less client's claimed fingerprint to reach favorite acceptance policy. HTTP has the same claimed-fingerprint fallback. This is a source-derived authorization concern, not an end-to-end reproduced bypass or a vulnerability finding against installed `1.18.2-1`. Storage permission and actual transfer behavior remain untested. Do not recommend favorites as unconditionally spoof-proof; a future epic must demonstrate rejection on both fallback paths before relying on verified-peer claims.

### Favorite enrollment and transfer tokens

Favorites persist a fingerprint, but the inspected add-favorite dialog probes an entered address through the unpinned discovery client and saves the result. It does not require an out-of-band key comparison or mutual enrollment decision. Existing favorites can update routing and alias without replacing their saved fingerprint. This offers key continuity where later transport checks apply, not verified personal ownership at first enrollment. [Favorite model][favorite] [Favorite dialog][favorite-dialog] [Favorite store][favorites]

Discovery fields such as alias, IP, port, device type, protocol, and `download` describe a route or advertised capability. None grants permission to upload. In v2, `prepare-upload` consults authorization and returns a session ID plus per-file tokens only for accepted files. Upload checks session ID, source IP, file ID, token, and pending state. The inspected active-session check is token/IP based, not another comparison with the original sender certificate. These are scoped session capabilities, not durable account identity or a reason to skip enrollment. [Protocol][protocol] [Upload handler][server-v2]

## Option C threat model

These are architectural assessments, not measured attacks. Assume an attacker may join or compromise a device on the LAN, advertise arbitrary discovery data, run a malicious AP, replay radio advertisements, or relay traffic. Endpoint key theft and a compromised receiver OS remain outside what transport authentication alone can solve.

| Proposed signal or threat                    | What it establishes and what it does not                                                                                                                                                                                                      |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Same LAN or private IP range                 | Reachability/topology, not ownership. Guest devices, compromised IoT peers, VPN bridges, and routed private networks can satisfy it. A same-LAN attacker can send offers and spoof discovery labels.                                          |
| Approved SSID                                | A network name is reproducible. An evil-twin AP can advertise it. Shared Wi-Fi credentials do not identify an individual sender.                                                                                                              |
| Approved local network profile               | Stronger than SSID string equality when tied to the OS's authenticated connection configuration. Still authenticates network access or an AP, not the sender's application key. A compromised approved AP/network retains that context.       |
| Bluetooth advertisement, name, address, RSSI | A proximity hint, not authenticated ownership or reliable distance. Names/advertisements can be copied, addresses rotate, radio strength varies, and relay defeats simple "nearby" checks.                                                    |
| Authenticated Bluetooth pairing              | Can prove possession of a Bluetooth pairing key, but only identifies the Quick Share/LocalSend peer if an explicit authenticated cross-transport binding exists. Seeing a paired device nearby does not authenticate an unrelated LAN socket. |
| LAN plus SSID plus Bluetooth                 | Correlated context observations, not independent identity factors. The attacker can satisfy network conditions and replay/relay radio presence without holding the enrolled application key.                                                  |
| Compromised AP with verified application TLS | Can deny service, observe traffic timing, or redirect discovery. It should not impersonate an already verified key if fingerprint/session checks fail closed. It can still attack first-use enrollment.                                       |
| Lost enrolled device                         | Possession of the stored private key may authorize future transfers. Context does not revoke it. Provide local removal and define what offline revocation can and cannot achieve.                                                             |

Use context to narrow verified-peer policy, for example "accept from this verified peer only while connected to this approved profile." Require fresh locally observed context; unknown, stale, disconnected, or ambiguous state disables automatic acceptance. Never infer a sender's network membership solely from fields the sender reports. Do not require continuous Bluetooth scanning unless the chosen policy and platform evidence justify its power, permission, and reliability costs.

## Go/no-go for the independent future epic

Two distinct products need separate acceptance decisions:

1. **Verified-peer auto-accept.** Explicit enrollment binds a durable key to the intended peer through an authenticated exchange or out-of-band comparison. Every later session proves possession before authorization; context may impose additional restrictions. A favorite label alone is insufficient enrollment evidence.
2. **Less-secure convenience acceptance.** Explicit opt-in authorizes a broader reachable population within stated contexts. Default off, clear warning, visible active state, bounded duration and easy disable. No "trusted," "your devices," or multi-factor claims. Automatic opening/execution of received content remains a separate, default-off choice.

Go only after recording the exact LocalSend releases/platforms, supported transport and direction, enrollment UI, key storage/reset/rotation/removal behavior, and receiving policy. Demonstrate that the chosen installed builds have the checks being relied on. Do not assume the current upstream commit describes every device running the same product name.

No-go for verified-peer claims if HTTP downgrade, absent client certificates, a JSON fingerprint, discovery/name rematching, unverified first-use enrollment, or key replacement can silently authorize a peer. No automatic acceptance work belongs in the current panel/settings epics.

Future proof must cover both directions and composed UI-to-control behavior with fake external processes and actual resulting snapshots, not source-string assertions or mock-call echoes. Separately require real TLS/peer evidence for the security claims. Required journeys include:

- Verified favorite succeeds without a repeated prompt only under the selected policy; unknown peer keeps manual consent.
- Same alias/IP with another key, spoofed fingerprint, changed/expired/revoked key, HTTP fallback, and certificate-less browser mode cannot inherit verified acceptance.
- First enrollment under an active MITM fails verification; cancelling enrollment saves no authorization.
- PIN absent/wrong/blocked/correct remains distinct from consent; text offers do not accidentally inherit file-only quick save.
- Context enters, leaves, becomes stale, or changes during preparation; authorization rechecks before acceptance and does not downgrade on uncertainty.
- Replayed tokens, cross-session uploads, changed transport, and reconnects cannot reuse unintended authorization.
- Receiving browser uploads and serving browser downloads have independent consent and default-off auto-accept controls.
- Restart preserves the intended saved policy and key records; reset/removal stops automatic trust and does not silently reenroll by name.

## Sources

[website]: https://localsend.org/
[readme]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/README.md
[protocol]: https://github.com/localsend/protocol/blob/62bd3406ec80d62f2ed46269cdc06c4dcc391083/README.md
[persistence]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/persistence_provider.dart
[migration]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/persistence_provider_migrations.dart
[settings]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/settings_provider.dart
[modes]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/model/persistence/quick_save_mode.dart
[security]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/security_provider.dart
[cert]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/crypto/cert.rs
[http]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/http_provider.dart
[send]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/network/send_provider.dart
[receive]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/network/server/controller/receive_controller.dart
[send-controller]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/network/server/controller/send_controller.dart
[client]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/client/mod.rs
[client-v2]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/client/v2.rs
[server-cert]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/client/server_cert_verifier.rs
[client-cert]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/server/common/client_cert_verifier.rs
[server]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/server/mod.rs
[server-v2]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/server/v2.rs
[pin]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/http/server/common/pin.rs
[discovery]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/core/src/discovery/mod.rs
[favorite]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/model/persistence/favorite_device.dart
[favorite-dialog]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/widget/dialogs/favorite_edit_dialog.dart
[favorites]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/favorites_provider.dart
[rust-bridge]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/localsend_isolates/rust/src/api/server.rs
[isolate-bridge]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/packages/localsend_isolates/lib/src/isolate/child/server_isolate.dart
[server-provider]: https://github.com/localsend/localsend/blob/6279d3e30d1d1290caee3b81549f8128a8b01d9f/app/lib/provider/network/server/server_provider.dart
