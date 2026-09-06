# Your devices, preferred peers, and authentication

Research date: 2026-09-06. Research and proposal only; no trust policy change is approved here.

## Answer

Google's "Your devices" means devices using the same Google Account, not devices the user has locally starred. Android Help explicitly says these devices automatically accept transfers. Windows Help independently documents the same account condition and describes PIN comparison when a PIN appears. These are official product claims, not conclusions drawn from third-party clients. [Android Help][android] [Windows Help][windows]

Skipping the PIN and skipping receiving consent are separate decisions. In Google's pinned Nearby implementation, successful paired-key authentication clears the displayed PIN for either self-share or mutual-contact peers. Automatic receiving consent additionally requires self-share. An unverifiable paired key clears the self-share flag and keeps the manual path. Therefore "no PIN" does not imply "automatically accept any contact." [ShareSession][session] [IncomingShareSession][incoming]

The current Rust endpoint has neither account-backed identity nor saved authenticated peer keys. Its pin is a single preferred outbound discovery identifier. Retain manual authentication and receiving consent; do not relabel that preference "Your devices" or "Trusted devices."

## What the terms actually mean

| Term                                   | Security meaning                                                                                                                                                                                                                                                                                             |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Four-digit PIN                         | A short authentication string derived from the current UKEY2 handshake. Explicit comparison on the intended devices authenticates that session; merely displaying or remembering the digits does not. It is not a reusable password. [UKEY2][ukey2] [ShareSession][session]                                  |
| Receiving consent                      | Authorization to receive the introduced content. The local recipient can reject a cryptographically authenticated sender. Google's incoming flow separates introduction validation, confirmation, and `AcceptTransfer`. [IncomingShareSession][incoming]                                                     |
| Discovery name and endpoint ID         | Advertised presentation and routing data. The local LAN path decodes them before connecting and does not authenticate ownership. A familiar name is not evidence of identity. [Local discovery][local-discovery]                                                                                             |
| Account-mediated identity              | Certificates distributed through account-aware services, then used to authenticate the current connection. A peer-supplied account name or `for_self_share` claim alone is not that process. [Certificate schema][schema] [Certificate manager][manager]                                                     |
| Local pinned preference                | This project's saved identifier used for selection and automatic initiation of queued outbound content. It contains no verification result or public key. [Config][config] [Lifecycle][lifecycle] [Production][production]                                                                                   |
| Saved authenticated long-term identity | A future trust record that binds an explicitly verified enrollment to a key or credential, with proof of possession on later sessions and defined replacement/revocation. No such implementation was found in the inspected production path. A previous transfer or PIN comparison alone does not create it. |

## Primary-source mechanism

All Google Nearby source links below pin repository revision `588531995decf09500870ed4d2e1ac6740a3e338`. UKEY2 pins `10fc737aa901e873a3367e7e26b88eb01cd55d69`. Help pages were read on the research date and are mutable. These source observations are not proof that every current proprietary Android/GMS or Samsung build has identical implementation details.

1. `NearbyShareCertificateManagerImpl` generates separate self-share and contact certificates. `DownloadPublicCertificatesInExecutor` requires a current account and device ID, then retrieves credentials through the identity RPC client. `UploadDeviceCertificatesInExecutor` publishes credentials grouped under self/contact visibility. The manager maintains private/public storage and refresh schedules. This is materially more than a sign-in button. [Manager implementation][manager]
2. `PublicCertificate` contains a public key, secret identifier/key, validity interval, encrypted metadata, and `for_self_share`. The documented ownership field belongs to the distributed certificate, not an arbitrary discovery label. [Schema][schema]
3. `PairedKeyVerificationRunner::SendPairedKeyEncryptionFrame` signs the current raw authentication token with a role prefix using a private certificate. It also hashes that token using the remote certificate. Verification checks the remote signature and the token hash against applicable private certificates. Missing credentials use random placeholders and produce `kUnable`; failed signature verification with a known certificate produces `kFail`. [Runner][runner]
4. `ShareSession::ProcessKeyVerificationResult` rejects `kFail`, clears the PIN on `kSuccess`, and clears `self_share_` on `kUnable`. Its success comment explicitly includes self-share and mutual contacts. [Session][session]
5. `IncomingShareSession::ReadyForTransfer` publishes `kAwaitingLocalConfirmation` unless `self_share()` holds. `AcceptTransfer` remains the operation that sends the protocol acceptance. Authentication supports the product's lower-friction policy; it does not remove the authorization concept. [Incoming session][incoming]
6. Incoming non-Everyone visibility treats an unverified certificate hash as failure, and hidden visibility rejects incoming connections. A Linux preference cannot persuade a stock Android receiver to accept account-free traffic in its account-only mode. [Runner][runner]

Google's public account and certificate sources support the mechanism above. They do not establish an available, supported Linux OAuth enrollment contract, permission to reuse official-client credentials, or a current stock-Android interoperability guarantee for a new implementation. No credentials were requested, obtained, or configured.

## Current Rust evidence

CodeGraph was queried first for pinned preferences, paired-key exchange, configuration, and persistence. It flagged several changed-on-disk files; direct reads supplied those current sections. This was a source audit, not a running trust or phone test.

- `Config::pinned_peer_id` is `Option<String>`. `Config::save`, `set`, and `parse_toml` persist it in `$XDG_CONFIG_HOME/omarchy-quickshare/config.toml`. The inspected schema has no peer certificate, fingerprint, account token, or verified-enrollment field. [Config][config]
- `Daemon::persist_pin` saves only the supplied ID. `pin_if_configured` compares the sighting ID for string equality; `unpin_peers` clears live flags and the config value. Pinning does not require a successful connection or a user-confirmed authentication event. [Lifecycle][lifecycle] [Daemon request dispatch][daemon]
- `EndpointSnapshot::pin_peer` only checks that the peer exists and sets one flag. `Coordinator::queue_outbound_with_preference` selects that peer when present; otherwise it starts discovery. [Snapshot][snapshot] [Coordinator][coordinator]
- LAN `discovered_peer` obtains `peer_id` from `MdnsInstance::label()` and the name from endpoint-info property `n`. `MdnsInstance` encodes a four-byte endpoint ID with protocol constants. Neither operation verifies a durable peer key. Comments using "stable" describe the software's lookup identifier, not cryptographic or cross-restart stability. [Discovery][local-discovery] [Advertisement][advertisement]
- `Daemon::apply_network_event` applies the preference to `PeerSeen` and calls `auto_start_pinned`. `start_pinned_outbound` also selects a configured ID when content is queued. This preference already does more than sorting the list: it can initiate sending. [Production][production]
- `SharingSession::exchange_account_free_pairing` sends random 72-byte signed-data and six-byte hash placeholders, receives paired-key data, and sends `UNABLE`. `decode_pairing` accepts the `UNABLE` result; it does not verify signatures, import a certificate, or persist an authenticated relationship. Its encryption-frame branch maps presence to `PairingStatus::Unable`. This is fallback protocol participation, not enrollment. [Session][local-session] [Frames][frames]
- UKEY2 constructors use `OsRng` and initiator/responder handshakes per connection. The specification uses ephemeral keys and transcript-derived authentication. Saving an ephemeral UKEY2 public key or previous PIN would not identify the next connection. [Local session][local-session] [UKEY2][ukey2]
- `network::transfer::send_on_connection` publishes the PIN as `OutboundPairing`, completes account-free pairing, then calls the outgoing payload flow. There is no local "PIN compared" barrier in that function. Receiver acceptance is the subsequent authorization boundary, not proof that the local sender compared the PIN. [Transfer][transfer]

The last two preference observations matter. It would be inaccurate to claim the current pinned shortcut always waits for explicit local PIN verification. The source permits automatic initiation based on unauthenticated discovery data. A malicious receiver accepting an offer cannot authenticate itself by accepting it.

[INFERENCE] An attacker reproducing the preferred discovery identifier could attract queued content if no effective manual authentication check intervenes. This follows from the ID selection and transfer flow; no spoofing experiment or exploit was run. Audit this boundary before advertising pinned sending as safe one-click identity trust.

No evidence establishes that stock Android supplies this account-free endpoint with a stable authenticated key that can be enrolled once and reused indefinitely. Endpoint ID rotation, certificate renewal, and UKEY2 ephemeral keys are different lifecycles. This audit did not measure any phone's endpoint-ID rotation interval or cross-medium identifier equivalence.

## What can safely be made easier

Recommendation, subject to the later decision discussion:

- Keep one panel, a conspicuous session PIN, the introduced content summary, and an explicit receiving decision. Automatic panel presentation can remove navigation without accepting the content.
- Use the existing preference only as a convenience label such as "Preferred device." Prefer preselection over unattended initiation where the recipient has not been authenticated. Do not silently rematch a missing preferred ID by device name or MAC address.
- Decide how the sender explicitly verifies the current PIN before content leaves. Automatic UI selection and an easy cancel button are not substitutes for that check. No PIN-bypass work belongs in this UX round.
- Keep account-free public discovery. The proposed inbound On/Off control cannot be represented as Google's "Your devices" mode, and outbound searching cannot grant account-only visibility access on Android.
- A one-time PIN comparison can safely authenticate an enrollment exchange only if that exchange actually binds a durable credential and both sides subsequently prove possession. The current exchange creates no such record. Explicitly choosing "trust" after seeing a name, or trusting the first key silently, is not verified enrollment.

The first two UI improvements need no Google account and do not require changing Android. A genuinely lower-friction future authentication scheme does require more than Linux-only preference storage.

## Native QR sharing deserves a separate evaluation

Android Help documents a native per-transfer alternative when discovery is awkward: the sender opens Quick Share and selects "Use QR code"; a nearby Android receiver scans it with its camera or Quick Settings scanner. The receiving instructions say scanning automatically connects to the sender to receive the content, with Wi-Fi and Bluetooth enabled. This is a supported Android user flow and a plausible way to reduce target-finding and confirmation friction without a companion app. It is not documented as durable device enrollment, same-account identity, or authorization for later transfers. [Android Help][android]

Treat the scan as an explicit per-transfer user action. The help page does not specify which authentication material the code binds, its replay/expiry rules, or the wire steps needed for a third-party Linux sender or receiver. It also does not establish that every scan path suppresses a PIN. A QR containing only an unauthenticated endpoint address would not, by itself, replace session authentication.

This repository's `account_free_encryption` sets `qr_code_handshake_data` to `None`; the feasibility scope excludes QR send flows. No implemented or measured stock-Android QR interoperability was established in this audit. A future bounded investigation should identify Google's exact QR payload and session binding, then prove Linux-to-Android and Android-to-Linux behavior independently before proposing a panel action. Do not offer a decorative QR or claim PINless compatibility from the help-page UX alone. [Local frames][frames] [Rust feasibility](../../rust-reimplementation-feasibility.md#what-can-be-left-out)

The same Android help page separately describes QR links for devices without direct AirDrop interoperability, with internet access, end-to-end encrypted uploads to Google servers, and 24-hour availability. That hosted delivery mechanism is distinct from nearby Android QR connection and is not evidence that this account-free Linux endpoint can use either route. Neither route turns the current pinned preference into authenticated persistent trust. [Android Help][android]

## Future options, not accepted scope

| Option                             | Prerequisite and limitation                                                                                                                                                                                                                                                                                                                                                                              |
| ---------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Keep account-free Quick Share      | Lowest complexity. Reduce navigation and selection steps, retain session authentication and consent. Current-phone compatibility still requires separate direction-specific evidence.                                                                                                                                                                                                                    |
| Account-mediated self-share        | Research supported enrollment, account access, device registration, certificate exchange, secure credential storage, expiry, logout, and revocation. Use server-issued ownership context plus session verification. Do not assume the open source grants public backend access. Stock Android may interoperate only after its account/certificate expectations are genuinely met; this is unproven here. |
| Explicit local key enrollment      | Requires a peer that exposes a durable credential, binds it over an explicitly verified session, and proves possession on later sessions. Current unmodified Android support was not established. A companion app or controlled peer protocol would be a different product scope, not transparent stock Quick Share interoperability.                                                                    |
| Import/cache observed certificates | Not a safe shortcut by itself. Provenance, owner binding, proof of possession, expiry, replacement, and revocation all remain necessary. No supported stock-Android account-free import/export/enrollment contract was established.                                                                                                                                                                      |

Google's schema also contains a device-pair `binding_id`, and its manager has binding-related RPC methods. Their existence does not prove an account-free, user-verifiable enrollment flow or a supported third-party API. Investigate only if the user chooses that future scope. [Schema][schema] [Manager][manager]

## Threat model and required failure behavior

| Threat                                         | Required boundary for any future trust feature                                                                                                                                                                                                                                                                                                      |
| ---------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Spoofed name, endpoint ID, mDNS record, or MAC | Presentation/routing never grants trust or automatic acceptance. Reverify the current connection; do not substitute matching labels for a signature.                                                                                                                                                                                                |
| Active man-in-the-middle                       | Compare the current PIN out of band or validate a credential bound to the current session. Do not treat encryption alone as intended-peer authentication. [UKEY2][ukey2]                                                                                                                                                                            |
| Captured session or changed ephemeral keys     | Old PIN/session keys do not authorize a new session. A reconnect needs new authentication; durable enrollment must authenticate the new transcript.                                                                                                                                                                                                 |
| Certificate/key rotation                       | Never silently replace an enrolled key because the name matches. Accept only an authenticated rotation mechanism or require new explicit enrollment. Google manages validity and scheduled replacement, rather than pinning one certificate forever. [Manager][manager]                                                                             |
| Removed contact, logout, or revoked device     | Invalidate local authorization and cached trust according to a defined policy. Google regenerates private certificates after reported contact removal, expires certificates, and documents clearing public certificates on logout. Instant revocation across offline devices is not proven. [Manager][manager] [Manager contract][manager-contract] |
| Lost or compromised authorized device          | Possession of a valid credential may let the thief act as the enrolled device. Provide explicit local revocation and account/device removal guidance if account support is added. Do not promise instantaneous remote invalidation or that auto-open is safe merely because identity is known.                                                      |
| Spoofed queued-send destination                | Require intended-peer authentication before sending user content; remote acceptance alone does not establish it. Preserve cancel/reject and never silently transfer to a similarly named replacement.                                                                                                                                               |

## Implementation owners and later acceptance scenarios

These identify the eventual boundaries; they are not instructions to implement now.

| Owner                                                                                 | Narrow responsibility                                                                                                                                                                           |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `packaging/omarchy-plugin/`                                                           | Preferred-device wording, visible PIN comparison guidance, selection and receiving-consent presentation. Do not store or decide peer trust in QML.                                              |
| `crates/app/src/config.rs`, `daemon/lifecycle.rs`                                     | File-backed preference persistence. Keep preference separate from a future authenticated-identity store.                                                                                        |
| `crates/app/src/daemon/production.rs`, `daemon/network/transfer.rs`                   | Automatic selection versus actual sending; any explicit sender-verification transition must prevent content release until satisfied.                                                            |
| `crates/core/sharing/src/coordinator.rs`, `protocol/session.rs`, `protocol/frames.rs` | Consent/authentication state and typed verification results; never fabricate paired-key success.                                                                                                |
| `crates/core/crypto`                                                                  | Session authentication primitives, not account ownership or UI preferences.                                                                                                                     |
| Future account owner                                                                  | Account/contact visibility is a new core concern under the existing project-structure policy. Do not add speculative account machinery during this panel change. [Project structure][structure] |

Behavioral acceptance proposals for a later implementation:

1. Pin and restart: the preference returns, but no "verified" or "your device" identity claim appears. An inbound offer from that ID still requires consent.
2. Duplicate name or preferred ID: an unauthenticated peer cannot gain silent trust; no payload leaves before the chosen session-authentication requirement is satisfied.
3. Current PIN mismatch or reject: the share terminates without payload release. An old matched PIN cannot approve a new session.
4. Preferred peer disappears or rotates ID: the panel returns to selection rather than silently selecting the same name. Unpinning removes only the preference, not an imaginary account relationship.
5. Account-free paired-key `UNABLE`: retain the manual path, never interpret it as success. Known-credential authentication failure, if later implemented, must not downgrade silently to trusted acceptance.
6. Any future enrollment: explicit verification binds the durable key; later proof of possession is checked. Changed, expired, revoked, or missing credentials stop automatic trust and require the specified recovery decision.

Unresolved decisions are whether to keep automatic preferred initiation, what exact sender-verification interaction is required, and whether Google account support or a controlled-peer enrollment product is worth its lifecycle burden. No account/trust extension is necessary to deliver the requested unified panel.

## Evidence limits

Only repository/source and official documentation reads were performed. No builds, tests, linters, formatters, discovery, visibility, clipboard actions, account setup, credential access, or physical-peer tests ran. Existing simulator successes are not stock-Android self-share evidence; the [paired-key audit][audit] and [connection-testing policy][testing] already distinguish those boundaries.

[android]: https://support.google.com/android/answer/9286773?hl=en
[windows]: https://support.google.com/android/answer/13801258?hl=en
[ukey2]: https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/README.md
[runner]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/paired_key_verification_runner.cc
[session]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/share_session.cc
[incoming]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/incoming_share_session.cc
[schema]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/proto/rpc_resources.proto
[manager]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/certificates/nearby_share_certificate_manager_impl.cc
[manager-contract]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/certificates/nearby_share_certificate_manager.h
[config]: ../../../crates/app/src/config.rs
[lifecycle]: ../../../crates/app/src/daemon/lifecycle.rs
[daemon]: ../../../crates/app/src/daemon.rs
[production]: ../../../crates/app/src/daemon/production.rs
[transfer]: ../../../crates/app/src/daemon/network/transfer.rs
[local-discovery]: ../../../crates/app/src/daemon/network/worker.rs
[advertisement]: ../../../crates/core/sharing/src/protocol/advertisement.rs
[local-session]: ../../../crates/core/sharing/src/protocol/session.rs
[frames]: ../../../crates/core/sharing/src/protocol/frames.rs
[snapshot]: ../../../crates/core/sharing/src/snapshot.rs
[coordinator]: ../../../crates/core/sharing/src/coordinator.rs
[structure]: ../../architecture/project-structure.md
[testing]: ../../connection-mocking-tools.md
[audit]: ../simulator-vs-android-paired-key.md
