# Google account enrollment for self-share

Status: needs-triage
Research date: 2026-09-06
Scope: option A, research only. No implementation or account setup is authorized.

## Decision

Google sign-in on third-party Linux is technically possible through a documented Desktop app OAuth flow. Google explicitly includes Linux in its loopback redirect guidance. That establishes a supported way to authenticate a user to our application, subject to Google's policies. It does not register a Quick Share device or authorize access to its certificate services. [Desktop OAuth][desktop] [Loopback support][loopback]

Account-backed Quick Share self-share is a real Google product capability. Android Help says transfers between devices using the same Google Account are automatically accepted, and its "Your devices" visibility uses that account relationship. The help page lists Android, Chromebook, and selected Windows support, not a Linux client enrollment program. [Android Help][android]

Recommendation: keep A as a gated research ticket, not a promised Google sign-in feature. No published third-party Linux Quick Share enrollment API, applicable enrollment scopes, or permission to use its account backend was established in this review. This is not proof that such access can never be granted. It is insufficient evidence to build or ship the integration.

## Evidence by boundary

| Boundary                             | Finding                                                                                                                                                                                                | Evidence type                                                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------- |
| Ordinary Google authentication       | Documented for Desktop app clients, including Linux. Use our registered client, the system browser, authorization-code flow with PKCE, and an appropriate loopback redirect.                           | Official developer documentation, not a runtime test. [desktop] [loopback]                                        |
| Identity returned to our application | OpenID Connect documents `openid`, `email`, and `profile`. An ID token's `aud` must identify our application; `sub` identifies the user. This is an identity assertion, not a Quick Share certificate. | Official identity contract. [oidc] [scopes]                                                                       |
| Account-backed device credentials    | Google's source gates certificate operations on account/device context and publishes or queries credentials through an identity RPC client. Self-share has its own visibility bucket.                  | Pinned first-party source, not a public access contract. [manager] [schema]                                       |
| Supported third-party enrollment     | No registration guide, service enablement procedure, enrollment scope, or certificate REST/RPC contract was found in the reviewed public documentation.                                                | Bounded negative finding, not proof that no private or partner program exists. [desktop] [scopes] [nearby]        |
| Stock Android interoperability       | Google's own same-account flow is documented. A new Linux implementation authenticating as the same owner's device in both directions remains unproven.                                                | Product documentation only. No phone/account experiment ran. [android]                                            |
| Permission                           | Google's user-data policy explicitly says not to use undocumented APIs without express permission. OAuth consent alone does not supply that permission.                                                | Published policy requirement; applicability and any additional agreement need confirmation. [data-policy] [terms] |

## What the documented OAuth path provides

The public registration path is to create an application-owned Google Cloud project and an appropriate OAuth client. The native-app guide says to enable the APIs the application needs, then create credentials for that project. It specifically recommends the Desktop app type and loopback redirect for Linux. Those are documented prerequisites, not actions performed in this research. [desktop]

The documented authorization endpoint is `https://accounts.google.com/o/oauth2/v2/auth`; code exchange uses `https://oauth2.googleapis.com/token`. The public OpenID Connect contract also provides discovery metadata. These are authentication/authorization endpoints, not Nearby device-registration or certificate endpoints. [desktop] [oidc]

A successful sign-in would let our application validate an identity assertion intended for it. It would not prove that a nearby endpoint owns that account, bind the current sharing session to a device credential, or make stock Android recognize the Linux device under "Your devices". Those require the separate enrollment and session-verification mechanism. Matching email text is not a substitute. [oidc] [manager] [Related trust audit][audit]

No enrollment scope should be guessed. The complete retrieved public OAuth scopes catalog was searched for Nearby and sharing-related entries. It contains OpenID identity scopes but no identified Nearby/Quick Share device-enrollment entry. Google's Nearby developer landing page offers Nearby Connections, deprecated Nearby Messages, and Fast Pair. It does not document Quick Share account enrollment. Nearby Connections transport access and Fast Pair device-manufacturer registration are not that contract. [scopes] [nearby]

This search did not inspect a signed-in Cloud API Library or a partner portal. A targeted search for official developer documentation about the Nearby sharing/identity backend and enrollment returned no relevant official result; relaxed results were not treated as evidence. The precise conclusion is "not established in the public sources reviewed," not "the server must reject every third-party client."

## What Google's published implementation establishes

These observations pin Google Nearby revision `588531995decf09500870ed4d2e1ac6740a3e338`.

- `NearbyShareCertificateManagerImpl` uses an account manager and device ID for account-backed certificate work. `UploadDeviceCertificatesInExecutor` uses `PublishDevice`; `DownloadPublicCertificatesInExecutor` uses the `QuerySharedCredentials` family through an injected `IdentityRpcClient`. [manager]
- Self-share uses `DEVICE_VISIBILITY_SELF_SHARE` and `PerVisibilitySharedCredentials::VISIBILITY_SELF`. Credentials are published and retrieved by visibility rather than deriving self-share trust from a discovery name. Schedulers handle certificate refresh, upload, download, and expiration. [manager]
- `PublicCertificate` describes key material, validity and encrypted metadata alongside `for_self_share` and `binding_id`. The schema also describes devices, contacts, and self-contact metadata. A flag's presence in a schema does not authorize a client to assert ownership. [schema]
- These inspected files do not supply a supported OAuth scope or third-party API enrollment procedure. They abstract the network endpoint and authentication through the RPC client. No hostname or wire route is asserted here merely from an internal method name. [manager] [schema]

The existing [trust audit][audit] separately traces paired-key proof over the current session. It records that successful authentication can clear the PIN while automatic receiving consent additionally depends on self-share. Starting a transfer process, signing in, and automatically accepting an incoming offer remain separate operations.

Published source makes parts of the mechanism inspectable. It does not show that an application-owned OAuth client can enable or call the required backend, or that its credentials will be accepted by current proprietary Android/GMS implementations.

## Permission and support questions

Google's API Terms, section 2.c, require access by the means described in the API's documentation, use of assigned developer credentials, and accurate client identity. Section 2.e distinguishes the open-source software license from service terms. The API user-data policy is more explicit: "Do not use undocumented APIs without express permission." It requires official documented means of accessing the API service. [terms] [data-policy]

The OAuth policies require an appropriate registered client, truthful branding, minimal scopes, secure handling of tokens, and a secure browser rather than an embedded user-agent under the developer's control. Production branding and sensitive/restricted scopes can require verification. User-data disclosures must describe actual access and use. Any enrollment proposal must identify its actual scopes and resulting requirements before promising a verification path. [oauth-policy] [data-policy]

Therefore:

- Do not reuse Google's or another application's client IDs, secrets, cookies, account tokens, or first-party identity. Do not imitate another platform to obtain access.
- Do not interpret an Apache source license as permission to use an undocumented hosted backend. Availability of protobuf definitions is not an API access agreement.
- Do not treat a consent screen, an accepted token, a source-level RPC call, or another project's working request as evidence of permission or maintained third-party support.
- Confirm whether Google offers a documented public or partner enrollment route for a third-party Linux client, and whether redistribution of the resulting client is covered. If access is undocumented, obtain express permission before attempting it.

These are project access gates grounded in published policies, not an unqualified legal judgment about all interoperability research. Applicable service-specific terms, any written permission, jurisdiction, and redistribution conditions remain questions for Google and, where needed, qualified advice. No Google representative was contacted.

## Third-party precedent, not authorization

The NearDrop maintainer's README describes a partial macOS implementation, LAN-only operation, and everyone visibility; it says limited visibility requires talking to Google servers. This corroborates the distinction between local transfers and account-backed visibility, but is not Google documentation or permission. [NearDrop README][neardrop]

The rquickshare maintainer's README describes Linux/macOS transfers and LAN limitations. The inspected README does not establish an authorized Google sign-in or self-share enrollment contract. Its general interoperability claim cannot fill that gap. [rquickshare README][rquickshare]

These READMEs were retrieved from their default branches on the research date and are mutable. They were not executed or audited for their current authentication internals. No claim is made that these projects could never implement enrollment, or that their published code confers backend access rights on this project.

## Research ticket acceptance and go/no-go evidence

The proposed ticket is research only and must not block the independent panel UX work. Ticket publication still requires approval of the breakdown under the local workflow.

1. Identify the Google-owned enrollment service and its applicable contract. Produce a public developer registration/API document, or express written permission covering this application's Linux client and intended distribution. Source code alone fails this gate.
2. Record the exact application-registration type, API enablement requirements, permitted scopes, endpoints, account/device registration methods, quotas, verification requirements, and any partner restrictions. Establish that our own client credentials are eligible. Do not create credentials or enable services as part of this documentation ticket.
3. Specify the complete identity chain: user sign-in, device registration, private credential generation/storage, certificate publication/download, server-mediated owner context, and proof bound to the current connection. Explain which party authorizes each step.
4. Define refresh, expiration, logout, token revocation, device removal, certificate rotation, reinstall, account change, offline operation, and failure behavior. Distinguish immediate local invalidation from any bounded or unknown remote revocation delay.
5. Only after access permission and separate experiment approval, demonstrate Linux-to-stock-Android and Android-to-Linux self-share using application-owned credentials. Record OS/GMS versions, "Your devices" visibility, authenticated session outcomes, consent behavior, and actual received payloads. An OAuth token or simulated peer is insufficient proof.
6. Include negative cases in that later experiment: different account, forged self-share flag, missing or expired credentials, revoked access, changed key, and failed paired-key proof. Authentication failure must never become silent trusted acceptance. Document manual fallback explicitly rather than silently downgrading.
7. Return a go/no-go decision with evidence. If access remains undocumented without express permission, stop backend attempts and mark implementation blocked. If permission exists but interoperability fails, report the exact failing boundary rather than shipping a sign-in button that implies self-share works.

Recommended outcome today: go for bounded documentary feasibility research; no-go for implementing or advertising account-backed self-share yet. Keep preferred-recipient settings distinct from authenticated "Your devices" identity.

## Provenance and limits

Official documentation was read on the research date and is mutable. The OAuth policy page reported modification on 2026-08-05, API Terms on 2021-11-09, and user-data policy on 2024-02-15. Google source links below are immutable revision references. Third-party README statements are maintainer claims, not official support guarantees.

Only public documentation, source reads, and existing local research were used. No account access, credential setup, hosted enrollment calls, live discovery, clipboard access, physical-peer transfer, installs, tests, builds, formatting, or validation ran. This note proves a documentary distinction and identifies missing evidence; it does not prove a working integration.

[desktop]: https://developers.google.com/identity/protocols/oauth2/native-app
[loopback]: https://developers.google.com/identity/protocols/oauth2/resources/loopback-migration
[oidc]: https://developers.google.com/identity/openid-connect/openid-connect
[scopes]: https://developers.google.com/identity/protocols/oauth2/scopes
[nearby]: https://developers.google.com/nearby
[android]: https://support.google.com/android/answer/9286773?hl=en
[oauth-policy]: https://developers.google.com/identity/protocols/oauth2/policies
[terms]: https://developers.google.com/terms
[data-policy]: https://developers.google.com/terms/api-services-user-data-policy
[manager]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/certificates/nearby_share_certificate_manager_impl.cc
[schema]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/proto/rpc_resources.proto
[neardrop]: https://github.com/grishka/NearDrop/blob/HEAD/README.md
[rquickshare]: https://github.com/Martichou/rquickshare/blob/HEAD/README.md
[audit]: ../trusted-devices.md
