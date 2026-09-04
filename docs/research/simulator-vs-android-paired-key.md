# Simulator versus Android paired-key behavior

Research date: 2026-09-04

## Conclusion

The green simulator results are real but narrower than the recorded Android
failure. The paired-key frame order and account-free `UNABLE` result agree with
Google's implementation. The first concrete mismatch sat one layer lower, in
Nearby Connections payload framing.

Before the local lifecycle change:

- Omarchy Quick Share sent each byte payload as one data packet containing the
  body and `LAST_CHUNK` together.
- Google's implementation and RQuickShare send a body packet with no terminal
  flag, followed by an empty `LAST_CHUNK` packet.
- Omarchy Quick Share accepted only `DATA` payload-transfer packets. Its
  generated protocol also defines `CONTROL` and `PAYLOAD_ACK`, and Google's
  payload manager handles all three.

The current local code implements that Google and RQuickShare BYTES lifecycle.
Non-empty bodies go out as `DATA` without `LAST_CHUNK`, then an empty terminal
`DATA` at `offset = size` with `LAST_CHUNK`. A valid `PAYLOAD_ACK` is ignored.
`PAYLOAD_ERROR` and `PAYLOAD_CANCELED` discard matching partial receive state
before becoming typed events. Sharing control frames use fresh payload IDs.
Attachment payload ID `3` is unchanged. Text cancellation decoding probes only
non-attachment IDs.

That local change is not a measured phone fix. The inbound physical run that
reached paired-key `receive_result` and then `connection_unexpected_frame` was
recorded against the old one-packet, `DATA`-only receiver. It proves the
received data failed in the Connections layer before the Sharing paired-key
result decoder. It does not identify the frame. No precise phone frame was
captured. Physical-phone interoperability has not been rechecked against the
new emission or receive path.

Capturing the rejected V1 frame type, payload packet type, and control event
is still the shortest route to proof.

## What Google and Android publish

Google's public [Nearby Connections overview][connections-overview] documents
advertising, connection establishment, and byte, file, and stream payloads. It
does not document Quick Share's paired-key wire sequence. The Android
[`Payload` API][android-payload] says each payload has a unique identifier, but
also stays above the transport-frame layer.

The open Google Nearby implementation supplies the missing detail:

1. [`PairedKeyVerificationRunner::Run`][google-pairing] sends paired-key
   encryption, reads the peer encryption, sends the local result, and then reads
   the peer result. An unverifiable account-free peer produces `UNABLE`, not
   `FAIL`.
2. [`ShareSession::WriteFrame`][google-session] serializes each Sharing frame as
   a new Connections byte payload.
3. [`Payload`][google-payload] gives each such payload a new sender-generated
   random ID.
4. [`PayloadManager::CreatePayloadChunk`][google-payload-manager] sets
   `LAST_CHUNK` only when the detached chunk is empty. A non-empty message is
   therefore emitted as a body data packet and a separate empty terminal
   packet.
5. The same payload manager dispatches `DATA`, `CONTROL`, and `PAYLOAD_ACK`.
   Control messages include payload error and cancellation.

The checked Google revision is the repository's pinned
`588531995decf09500870ed4d2e1ac6740a3e338`. This is primary implementation
evidence, but it is not proof that a current proprietary Android/GMS build has
identical flags and behavior.

## What RQuickShare does

RQuickShare revision `378d8ae969941bee4bf60ad34ac9cf8bb7005eb7`
independently implements the same paired-key order in its
[inbound][rquick-inbound] and [outbound][rquick-outbound] state machines:

- random 72-byte signed data and random 6-byte secret hash for account-free
  encryption;
- paired-key result `UNABLE`;
- a fresh random `i64` payload ID for every encrypted Sharing frame;
- a body data packet with flags `0`, followed by an empty packet at
  `offset = size` with `LAST_CHUNK`.

[NearDrop's protocol notes][neardrop-protocol] report the same two-packet
termination from Android. NearDrop is reverse-engineered corroboration, not an
official specification.

RQuickShare's code shows a practical implementation of the same framing. Its
repository does not provide a current-phone paired-key test or issue proving
compatibility with the exact phone/build used here, so it is not conformance
authority.

## Local divergence

Before the change, [`Connection::send_bytes`][local-transfer] sent one packet
with a non-empty body and `last = true`. [`Connection::payload`][local-transfer]
returned `UnexpectedFrame` whenever `packet_type` was not `DATA`, although the
[generated protocol][local-wire] defines `CONTROL` and `PAYLOAD_ACK` as valid
packet types.

[`SharingSession::exchange_account_free_pairing`][local-pairing] used payload
ID `1` for encryption and ID `2` for the result. Those IDs were distinct during
the failing pairing exchange, so duplicate ID reuse is **not** the cause proven
by that run. Fixed low IDs still differed from Google and RQuickShare's fresh
sender-generated IDs. ID `2` was later reused as the introduction payload ID,
which was a separate post-pairing lifecycle risk.

The current code matches the implemented contract below. Control frames call
[`next_control_payload_id`][local-control-id], which skips `0` and attachment
payload ID `3`. Text cancellation decoding only treats a BYTES event as cancel
when its ID is not the offer attachment ID.

## Why the simulated setup passes

| Evidence                   | Peer boundary                                           | What it proves                                       | Missing Android behavior                                              |
| -------------------------- | ------------------------------------------------------- | ---------------------------------------------------- | --------------------------------------------------------------------- |
| Rust unit and stream tests | Same Rust implementation on both ends                   | Internal ordering, decoding, and loopback transfer   | Both peers share one implementation, including two-packet termination |
| UKEY2 oracle               | Google UKEY2 cryptography                               | Secure-channel handshake                             | No Sharing paired-key or payload lifecycle                            |
| Android probe              | Nearby Connections public API                           | Discovery, connection, and generic payload transfer  | No stock Quick Share Sharing-layer exchange                           |
| Diverse LAN                | Current daemon against pinned Google-derived Linux peer | Independent full Sharing transfer in both directions | Not the current Android/GMS backend or feature set                    |

The Diverse LAN gate is important: it is not another Rust loopback. It runs the
current daemon against the pinned `kidfromjupiter/nearby` Linux peer in both
transfer directions. That peer's Google-derived payload receiver attaches the
body before completing a packet marked `LAST_CHUNK`, so it accepted the old
one-packet form and still accepts the two-packet form. Its success establishes
compatibility with that pinned, lenient peer, not equivalence with current
Android.

The local unit peers are self-consistent rather than independent. The oracle
and Android probe stop below or above the failing Sharing/Connections seam.
Together these tests can all pass while a stock Android Quick Share endpoint
sends or expects a packet variant the suite never drives.

The outbound URL attempt is a separate gap. It cancelled during production
peer-route selection and never reached paired-key exchange, so it provides no
paired-key evidence.

## Facts, inference, and unknowns

### Proven

- The physical inbound run reached paired-key `receive_result` and then returned
  `connection_unexpected_frame`. That run used the pre-change one-packet emitter
  and `DATA`-only receiver.
- That pre-change byte-payload emission differed from Google, RQuickShare, and
  NearDrop's observed two-packet termination.
- That pre-change payload dispatch rejected valid `CONTROL` and `PAYLOAD_ACK`
  packet types.
- The current local code implements the two-packet BYTES lifecycle, ignores a
  valid `PAYLOAD_ACK`, and turns `PAYLOAD_ERROR` and `PAYLOAD_CANCELED` into
  typed events.
- The green test set does not exercise this seam against the current stock
  Android Quick Share implementation.
- No precise phone frame was captured. Physical-phone interoperability has not
  been rechecked after the local change.

### Inference

On the recorded run, Android most likely sent `CONTROL` with a payload error,
`PAYLOAD_ACK`, or another valid Connections frame after receiving one of our
paired-key payloads. The old receiver would reject that frame before it could
read the peer's Sharing `PairedKeyResult`. A payload error caused by
body-plus-`LAST_CHUNK` was the leading mechanism because both Google and
RQuickShare emit a separate empty terminal packet. The evidence does not
identify the actual packet type or prove that Android rejects
body-plus-`LAST_CHUNK`. It also does not prove that the new local lifecycle
changes the phone result.

### Unknown

- The rejected V1 frame type and payload packet type.
- Whether Android objected to chunk termination, payload ID policy, another
  header field, or an unrelated Connections extension.
- Which Android/GMS feature or version differs from the pinned Linux reference
  peer.
- Whether a current phone still fails, and at which paired-key step, against
  the implemented lifecycle.

## Implemented source contract

These are local source commitments, not a claim about the rejected phone frame
and not a claim that a physical phone now interoperates.

Non-empty BYTES payloads emit `DATA` body flags `0`, then empty `DATA` at
`offset = size` with `LAST_CHUNK`. A valid `PAYLOAD_ACK` is accepted and
ignored when the endpoint does not track acknowledgements. `CONTROL`
`PAYLOAD_ERROR` and `PAYLOAD_CANCELED` become typed events or outcomes, not
`UnexpectedFrame`. Sharing control frames use a fresh sender-generated ID.
Attachment payload ID `3` stays correlated to introduction metadata. Text
cancellation decoding only probes non-attachment IDs.

`CONTROL` and `PAYLOAD_ACK` remain candidates for the recorded phone failure
until logs record the rejected V1 frame type, payload packet type, and control
event.

## Next proof steps

1. Log only the rejected V1 frame type, payload packet type, payload ID, and
   control event. Do not log encrypted payload contents or peer identifiers.
2. Re-run the physical inbound case first against the implemented lifecycle.
   Then run the independent Diverse LAN gate to detect regressions. Test
   production-discovered outbound route selection separately.

[android-payload]: https://developers.google.com/android/reference/com/google/android/gms/nearby/connection/Payload
[connections-overview]: https://developers.google.com/nearby/connections/overview
[google-pairing]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/paired_key_verification_runner.cc
[google-payload-manager]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/payload_manager.cc
[google-payload]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/nearby_connections_types.h
[google-session]: https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/share_session.cc
[local-control-id]: ../../crates/core/sharing/src/protocol/session.rs#L16-L29
[local-pairing]: ../../crates/core/sharing/src/protocol/session.rs#L127-L150
[local-transfer]: ../../crates/core/connections/src/session/protocol/transfer.rs#L37-L205
[local-wire]: ../../crates/core/wire/src/generated/location.nearby.connections.rs#L613-L645
[neardrop-protocol]: https://github.com/grishka/NearDrop/blob/master/PROTOCOL.md
[rquick-inbound]: https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/inbound.rs
[rquick-outbound]: https://github.com/Martichou/rquickshare/blob/378d8ae969941bee4bf60ad34ac9cf8bb7005eb7/core_lib/src/hdl/outbound.rs
