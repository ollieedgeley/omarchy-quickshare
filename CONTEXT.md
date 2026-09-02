# Omarchy Quick Share

This context describes an account-free Omarchy endpoint that exchanges content with Android Quick Share peers. It sends and receives files, plain text, and URLs.

## Language

**Local endpoint**:
The Omarchy side of a Quick Share exchange. It is the receiver during an inbound share and the sender during an outbound share; neither role defines the product.
_Avoid_: Server, Linux client

**Peer**:
The remote Quick Share endpoint participating in a connection.
_Avoid_: Phone, sender device, receiver device

**Medium**:
A local carrier used to establish or continue a connection, such as BLE, Bluetooth Classic, LAN, hotspot, or Wi-Fi Direct.
_Avoid_: Transport, radio

**Connection**:
An authenticated Nearby Connections relationship with a peer that may move between media without changing identity.
_Avoid_: Socket, link

**Upgrade**:
The negotiated move of an active connection to a higher-bandwidth medium while preserving its session and payload state.
_Avoid_: Reconnect, migration

**Share**:
The consent-driven Quick Share exchange carried by one connection.
_Avoid_: Session, job

**Inbound share**:
A share initiated by a peer and accepted or rejected by the local endpoint.
_Avoid_: Receive session, download

**Outbound share**:
A share of one or more supported attachments initiated by the local endpoint and accepted or rejected by a peer.
_Avoid_: Send job, upload

**Attachment**:
One declared item in a share, such as a file, text value, URL, or Android application package.
_Avoid_: Payload, content item

**Payload**:
The Nearby Connections byte transfer identified independently from the attachment that consumes it.
_Avoid_: Attachment, file

**Visibility window**:
A bounded period during which peers may discover the local endpoint for an inbound share. Outbound discovery does not open a visibility window.
_Avoid_: Everyone mode, pairing mode

**Oracle**:
The pinned Google implementation used to report expected protocol behavior through the project test protocol.
_Avoid_: Simulator, mock peer

**Simulator route**:
A reproducible connection path through a simulator or virtualized operating-system service that has passed its own reference self-test.
_Avoid_: Device test, mock
