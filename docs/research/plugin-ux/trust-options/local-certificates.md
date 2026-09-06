# Local certificates and verified enrollment

Status: needs-triage
Research date: 2026-09-06. Option B, research only. No implementation or ticket publication is authorized.

## Answer

A custom certificate is useful only if the other endpoint authenticates it and proves possession of its own enrolled key on later connections. Creating a Linux certificate, installing a CA on Android, or remembering a successful transfer does not supply that relationship to stock Quick Share.

Google's public Nearby source contains a real session-bound signature mechanism, rotating sharing certificates, a deprecated offline certificate-exchange message, and backend-mediated device bindings. These are concrete leads, not proof of a supported account-free enrollment operation on unmodified Android. Do not call local enrollment cryptographically impossible. The missing product capability is an authenticated way to obtain the phone's usable sharing credential, retain the relationship through rotation, and make the phone recognize the Linux credential in return. [runner] [wire] [schema] [manager]

Recommendation: keep B as an independent research epic with a stop/go gate. Native QR deserves a separate bounded per-transfer investigation within it. A controlled peer or companion app can implement an explicit durable-key enrollment protocol, but that is a different endpoint, not transparent interoperability with Android Quick Share.

## Evidence and what it proves

Google Nearby sources below are pinned to `588531995decf09500870ed4d2e1ac6740a3e338`; UKEY2 is pinned to `10fc737aa901e873a3367e7e26b88eb01cd55d69`. They describe public source, not a measured current proprietary GMS or Samsung implementation. Android Help was read on the research date and is mutable. Local Rust observations refer to the current working tree, which contains changes; they are not an immutable release claim.

| Source                                              | Observed mechanism                                                                                                                                                                                                                                                                                                          | Limit                                                                                                                                                                                                |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `PairedKeyVerificationRunner` [runner]              | Signs the current raw authentication token with sender/receiver role prefix. Verifies the remote signature against an already supplied decrypted certificate. Separately checks a token hash using applicable local certificate material. Combines local and remote results; failure dominates, inability prevents success. | A received signature is not the public key or its provenance. The six-byte hash is not durable identity or a certificate export.                                                                     |
| `NearbyShareDecryptedPublicCertificate` [decrypted] | Uses ECDSA-SHA256 to verify with the certificate public key. Its token hash uses the certificate's symmetric secret key. Certificate metadata is decrypted and authenticated using distributed material and advertisement data.                                                                                             | Matching encrypted metadata or knowing shared certificate material is not itself proof of the private signing key. Public-key pinning alone does not implement the whole mutual paired-key exchange. |
| RPC `PublicCertificate` [schema]                    | Public key, symmetric identity/decryption material, validity interval, encrypted metadata, `for_self_share`, and `binding_id`. Distribution follows contacts/account context.                                                                                                                                               | This is not a TLS CA certificate chain. Copying `for_self_share=true` from an untrusted peer cannot establish account ownership.                                                                     |
| Certificate manager contract [manager]              | Uploads local public certificates and downloads permitted public certificates through the Nearby server; signs with the currently valid private certificate; refreshes expiry; clears public certificates on logout.                                                                                                        | Neither a permanent device root nor a public third-party enrollment permission is established by this contract. Account enrollment is option A, not a Linux-only certificate trick.                  |
| `CertificateInfoFrame` [wire]                       | Can represent public certificates shared offline, including public key and validity.                                                                                                                                                                                                                                        | `CERTIFICATE_INFO` is explicitly marked "No longer used" and its field deprecated. The schema alone does not prove stock Android emits, accepts, persists, or trusts it now.                         |
| `BindingFrame` [wire]                               | Initiator obtains a binding ID through `InitiateBinding`; peer calls `JoinBinding`; frames carry certificate IDs to associate with the binding. The listed request type is `FILESYNC`.                                                                                                                                      | This is backend-mediated, not evidence of a generic local pairing API. Certificate IDs are not the full public credentials.                                                                          |
| UKEY2 [ukey2]                                       | Ephemeral Diffie-Hellman keys and fresh randomness produce transcript-derived authentication strings and session secrets. OOB verification authenticates that handshake.                                                                                                                                                    | Saving the previous PIN or ephemeral public key cannot authenticate the next session.                                                                                                                |

### What durable proof does an account-free remote actually provide?

The inspected Rust path does not establish any. `account_free_encryption` generates random 72-byte signed-data and six-byte hash placeholders, with `qr_code_handshake_data=None`. `account_free_result` sends `UNABLE`; `decode_pairing` maps a present encryption frame to `PairingStatus::Unable` and only accepts an `UNABLE` result. It does not verify a signature or import a certificate. [frames] [rust-session]

A stock phone may send a genuine signature when it has an applicable private certificate, but receipt of bytes alone does not establish that fact. The Google runner also substitutes random bytes when signing material is unavailable. An authenticated public key and successful verification of the current role-bound token are needed to distinguish proof from a placeholder. Even then, possession is tied to that credential's validity, not an indefinite physical-device identity. [runner] [manager]

CodeGraph was consulted first for `SharingSession exchange_account_free_pairing account_free_encryption TLS certificate transport` and the LAN connection path. It reported changed-on-disk files; direct reads supplied the affected Rust sections. This was source inspection, not an interoperability experiment.

## How verified enrollment would have to work

These are proof requirements, not invented APIs or an implementation proposal.

1. Obtain the intended phone's public credential through an authenticated channel. Candidates are an official authorized account distribution path, a phone-displayed fingerprint/QR that commits to the credential, or credential exchange inside a session whose current authentication string the user explicitly compared on both devices. First-key-seen, discovery names, endpoint IDs, MACs, and a certificate downloaded from an unauthenticated URL do not qualify.
2. Show that stock Quick Share actually exports or otherwise exposes the credential used for later signatures. A generic Android keystore certificate generated by another app is not necessarily accessible to Quick Share. A successful PIN comparison helps authenticate an exchange only if that exchange occurs and binds the durable key.
3. On every subsequent session, verify proof of possession over the fresh session token with role separation, using the enrolled credential and valid policy state. Do not accept a replayed signature or a remote `SUCCESS` assertion as local verification.
4. For mutual PINless recognition, establish the reverse direction too. The phone must learn an authentic Linux sharing credential and apply its actual paired-key/visibility policy. Linux verifying the phone is not evidence that the phone recognizes Linux.
5. Bind the enrollment to explicit local authorization. Record whether it permits incoming autoaccept, outbound authentication without manual PIN comparison, or both. Neither permission authorizes future automatic sends without the user's content/recipient decision. Opening received content remains a separate permission.
6. Establish expiry, revocation, rotation, reconnect, and cross-medium behavior before persisting a trust decision.

The deprecated offline certificate frame is worth investigating because an explicitly verified initial session could, in principle, authenticate exchanged credentials. Its current stock-Android behavior must be demonstrated before choosing that route. A companion that implements its own key exchange can satisfy these requirements for its own protocol; it does not automatically enroll its key into Google's Quick Share store. [wire]

## TLS and Nearby Sharing are different trust decisions

A self-signed TLS certificate can authenticate a controlled TLS service after its key is authentically pinned. A CA-signed certificate can bind a service name under its PKI policy. Neither establishes that the endpoint is the intended Quick Share phone or that Google regards Linux as the user's device.

The inspected local media composition opens Nearby Connections over byte streams and uses `Handshake`; the LAN route is a TCP address. Sharing then runs the paired-key exchange above. Adding HTTPS around a separate LAN endpoint would not implement those Quick Share frames or make stock Android use that endpoint. Installing a custom CA into Android's system/user trust store is not evidence that Quick Share consumes that store for its sharing-certificate verification. [media] [runner]

Do not confuse a TLS transport fingerprint from another protocol, including LocalSend, with a Nearby Sharing credential. Evaluate that protocol independently.

## Consent authority in both directions

| Direction              | What Linux may decide                                                                                                                                                             | What still belongs to Android                                                                                                                  |
| ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Android sends to Linux | Linux can choose to autoaccept after it authenticates the sender and checks an explicitly enabled local policy. Incoming auto-open only presents the panel; it is not autoaccept. | Android owns its sending UI and its authentication of the receiver. Linux cannot promise the phone will suppress its PIN or confirmation.      |
| Linux sends to Android | Linux can replace manual PIN comparison only with proven intended-receiver authentication, then initiate the selected transfer.                                                   | Android owns receiving consent and visibility. It must send the protocol acceptance. A Linux certificate/preference cannot force that outcome. |

In the pinned Google source, successful paired-key verification clears the PIN for self-share or mutual contacts. `UNABLE` clears self-share; failure rejects. `IncomingShareSession::ReadyForTransfer` waits for local confirmation unless self-share holds, and `AcceptTransfer` sends `ACCEPT`. Thus PINless mutual-contact authentication is not blanket autoaccept. Android Help documents automatic acceptance for devices on the same Google Account. [session] [incoming] [android]

## Lifecycle and cross-media requirements

- Credential validity must be checked for the current session, including a defined clock-skew policy. An expired key cannot remain automatically trusted merely because it was enrolled once.
- A rotated certificate needs authenticated continuity, authorized redistribution, or explicit reenrollment. Do not silently replace a key because its label, address, account-looking metadata, or discovery ID matches.
- Revoking a local enrollment must stop new automatic trust immediately on Linux and invalidate pending decisions before content release. Offline remote revocation cannot be promised instantaneous without a demonstrated mechanism. Logout, device loss, application reinstall, and credential-store reset need explicit outcomes.
- LAN, Bluetooth, Wi-Fi Direct, and hotspot addresses are routes, not identities. A fresh connection on any medium must prove the same authorized credential or authenticated successor. A bandwidth upgrade must remain bound to the authenticated connection; fallback must not silently downgrade identity requirements.
- Store private signing material separately from ordinary UI preferences with defined access and deletion rules. Retaining remote certificate secrets may enable recognition/tracking; specify retention and diagnostic redaction. Do not put credentials into `config.toml`, shell arguments, QR logs, or normal snapshots.

These are recommended security requirements. The certificate manager's scheduled tasks, current-valid-certificate operations, and logout clearing establish why a one-time forever pin is incomplete, but do not prove an offline revocation SLA. [manager]

## Native QR is per-transfer, not saved trust

Android Help supports "Use QR code" on the sending device and scanning on the nearby Android receiver, which automatically connects to receive the content. The scan is an explicit action for that transfer. It does not document durable enrollment, future autoaccept, or guaranteed PIN suppression for third-party Linux endpoints. The separate internet QR-link flow uploads encrypted files to Google servers with 24-hour availability; do not conflate it with nearby Android QR. [android]

The wire schema is stronger evidence than a decorative QR idea: `qr_code_handshake_data` documents a signature of the UKEY2 token for an incoming connection and an HKDF of the connection token and UKEY2 token for an outgoing connection. However, that comment does not give a complete QR URL/payload, KDF parameters, key provenance, expiry, replay policy, or current phone parser contract. The inspected paired-key runner does not populate or verify this field. Rust leaves it absent. [wire] [runner] [frames]

A bounded QR research output must identify those exact bytes and semantics, then separately prove Linux-display/Android-scan and Android-display/Linux-scan feasibility. Require the scanned material to bind the intended session, consume or expire it, reject substitution and replay, and show what each UI authorizes. A scan must not create a saved trusted-device record implicitly. If the necessary phone behavior is unavailable, report that direction unsupported rather than offering an endpoint-address QR as authentication.

## Proposed independent epic research tickets

These are breakdown proposals, not published tickets. Obtain approval before creating individual issue files. No runtime, account, or phone actions are authorized by this note.

| Proposed research ticket                           | Required output                                                                                                                                                                                                                                                                      | Dependency / stop gate                                                                                                             |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| B1: credential acquisition and stock-peer contract | Immutable source trace for active export/import or offline certificate exchange, exact credential provenance, phone-version scope, and whether private-key possession is available on later sessions. Explicitly distinguish unsupported documentation from failed runtime evidence. | First gate. If stock Android offers no demonstrated route, do not implement a Linux trust button. Account-mediated routes go to A. |
| B2: mutual proof and lifecycle                     | Direction-specific protocol/state contract for current-token verification, Android recognition of Linux, expiry/rotation/revocation, reconnect and media changes; identify each consent owner.                                                                                       | Needs B1's usable credential path. A local verifier alone cannot close an outbound Android-autoaccept claim.                       |
| B3: native QR per-transfer feasibility             | Exact payload and handshake derivation, authenticity source, expiry/replay bounds, supported scan direction and consent UX. A later separately authorized device experiment must record actual outcomes.                                                                             | Can run independently of B1. No durable-trust claim without a separate enrollment proof.                                           |
| B4: security and interoperability decision         | Evidence table separating public-source behavior, official product documentation, controlled-peer results, and real stock-phone results. Go/no-go recommendation for stock Quick Share versus separately scoped companion protocol.                                                  | Needs relevant B1/B2 or B3 results. Simulators alone cannot establish stock compatibility.                                         |

If later implementation is authorized, composed QML-to-CLI/control journeys must assert resulting authentication/consent snapshots and whether content was released, using fake external processes where appropriate. Cover explicit enrollment, fresh reconnect, both transfer directions, changed/revoked/expired keys, refusal, missing credential, replay, QR expiry, and media transition. Do not assert source strings or mock-call echoes. Those tests supplement, never replace, separately authorized stock-device compatibility evidence.

## Red-line failures

Reject any design that equates `UNABLE` with success; accepts remote `SUCCESS` without local proof; pins an unverified first key; trusts a name/network/Bluetooth context as identity; fabricates self-share; revives deprecated certificate exchange solely because it has a protobuf type; silently rotates credentials; reuses a previous PIN; bypasses Android consent; or turns a one-transfer QR scan into permanent trust.

A controlled-peer-only result is valid research, but must be labelled that way. The useful negative result is "no supported stock enrollment path established by these sources and these authorized experiments," not "custom certificates are impossible."

Only documentation/source reads and local CodeGraph queries were performed. No implementation, tests, builds, lint, format, installs, discovery, clipboard access, account actions, phone actions, or runtime validation ran.

[runner]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/paired_key_verification_runner.cc
[wire]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/proto/wire_format.proto
[schema]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/proto/rpc_resources.proto
[manager]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/certificates/nearby_share_certificate_manager.h
[decrypted]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/certificates/nearby_share_decrypted_public_certificate.cc
[session]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/share_session.cc
[incoming]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/incoming_share_session.cc
[ukey2]: https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/README.md
[android]: https://support.google.com/android/answer/9286773?hl=en
[frames]: ../../../../crates/core/sharing/src/protocol/frames.rs
[rust-session]: ../../../../crates/core/sharing/src/protocol/session.rs
[media]: ../../../../crates/app/src/daemon/media/connection.rs
