# Quick Share discovery lifecycle

Research date: 2026-09-03

## Decision

The daemon should not continuously scan for peers. It should run bounded,
on-demand outbound discovery when the user starts a share or explicitly asks to
search, then stop its discovery lease when the user selects a peer, the search
expires, the user leaves the picker, or a transfer reaches a terminal state.

Inbound availability is different. Keep it off by default, then expose an
explicit visibility window owned by the daemon. A short window is the practical
account-free default. A user may later opt into persistent visibility, but that
is a privacy and resource setting, not an accidental consequence of starting
the daemon or opening the panel.

The plugin owns user intent and presentation. It asks the daemon to start or
stop a bounded search, select a peer, and open or close visibility. The daemon
owns radio leases, deadlines, candidate expiry, connection hand-off, and cleanup.
The plugin must not call BlueZ or network APIs itself: it cannot safely clean up
after a panel crash, restart, or disappearing transfer.

## What Quick Share documents

Google's Quick Share help describes product visibility, not a public radio API.
On Android, Receive mode makes a device visible to anyone nearby while that
screen remains open, and leaving Receive mode stops that visibility. Contacts
visibility requires an unlocked, active screen. Everyone visibility returns to
the previous setting after 10 minutes. Same-account visibility can remain while
the screen is off. [Use Quick Share](https://support.google.com/android/answer/15728591?hl=en)

Google's Windows help also describes visibility settings, background receiving,
and notification-driven acceptance. It says that a background Quick Share app
can receive and notify, but gives no public account of its BLE, Bluetooth, or
Wi-Fi activity. [Quick Share for Windows](https://support.google.com/android/answer/13801258?hl=en)

These sources support an explicit inbound-visibility state with a timed public
mode. They do not establish the exact radio calls, scan intervals, medium order,
or lifecycle used by the stock Quick Share implementation.

## What Nearby Connections documents

Nearby Connections is an app API, not a Quick Share compatibility API. It pairs
`startAdvertising()` and `startDiscovery()` with matching service IDs, and
provides matching stop calls. Google says to stop both operations when no longer
needed. It also says discovery often stops once the desired peers have been
found because discovery performs heavy radio work and can make established
connections more likely to break. A discoverer may still request a connection to
an endpoint found before `stopDiscovery()`, but cannot find new endpoints until
it starts again. [Nearby Connections advertise and discover](https://developers.google.com/nearby/connections/android/discover-devices)

The pinned open Nearby Sharing implementation follows the same lifecycle shape:
its discovery method installs found and lost callbacks then calls
`StartDiscovery`; `StopDiscovery` clears its endpoint cache and listener before
calling the lower service's stop method. Its advertising wrapper separately
calls `StartAdvertising` and `StopAdvertising`. This is useful implementation
evidence, but not a public contract for stock Android Quick Share.
[NearbyConnectionsManagerImpl](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/nearby_connections_manager_impl.cc#L2457-L2738)

## Radio lifecycle evidence

Android's BLE guidance says scanning is battery-intensive, should stop after it
finds the desired device, must not loop indefinitely, and needs a time limit.
[Find BLE devices](https://developer.android.com/develop/connectivity/bluetooth/ble/find-ble-devices)

Android calls Classic Bluetooth discovery heavyweight. It advises cancelling it
before connecting because discovery impairs new connections and gives existing
ones less bandwidth and more latency. [BluetoothAdapter](https://developer.android.com/reference/android/bluetooth/BluetoothAdapter)

Wi-Fi Direct exposes the same lifecycle: peer discovery starts explicitly and
continues until a connection starts or a group forms; the API also exposes
`stopPeerDiscovery()`. [Wi-Fi Direct](https://developer.android.com/develop/connectivity/wifi/wifi-direct)

On Linux, BlueZ `Adapter1.StartDiscovery` starts a client discovery session and
may run inquiry, scanning, and name resolution. `StopDiscovery` releases only
that caller's session. Physical discovery stops only after every client's lease
has ended. Filters are merged across clients, so the daemon must filter results
it receives and must not assume it owns the adapter. [BlueZ Adapter API](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Adapter.rst)

## Recommended daemon policy

- Start outbound discovery only for an explicit share, explicit refresh, or
  automatic send to a pinned peer. Starting the daemon and opening an empty
  panel do not start a scan.
- Give each search a bounded deadline. The product should choose and test the
  number, rather than present Google's Android 10-minute public-visibility
  setting as a documented scan duration.
- Stop the daemon's discovery lease when a peer is selected or a connection
  attempt begins. Keep the selected candidate only for the ensuing attempt.
- Cancel discovery on timeout, cancellation, daemon shutdown, adapter failure,
  or transfer completion. Surface the reason through the control event stream.
- Treat inbound visibility as a separate daemon state. Its timer begins only
  after advertising/listening succeeds; failure to start must leave visibility
  off and report an error.
- Do not change BlueZ global `Discoverable` or `Connectable` properties for a
  normal transfer. BlueZ documents them as global settings intended for the
  settings application. Use an application-owned advertisement and discovery
  lease instead.

This policy favours privacy, battery use, and connection reliability over the
small latency benefit of a permanent scan. It still permits a deliberate
persistent inbound mode later, if its visibility and cleanup semantics are
clear.

## Required control boundary

The daemon should expose `start_discovery(deadline)`, `stop_discovery`,
`select_peer`, `open_visibility(deadline)`, and `close_visibility`. Snapshot and
event data should distinguish discovery requested, active, timed out, stopped,
and failed; visibility requested, active, expired, stopped, and failed; and
candidate found or lost. A simulated peer and fake clock can test all of these
transitions without a phone.

Real Quick Share work remains below this boundary: BLE and Classic discovery,
DNS-SD browsing, Wi-Fi Direct or hotspot management, advertising, connection
authentication, and medium upgrade. The control lifecycle must not assume that
the stock product uses the public Nearby Connections API, even though the public
API and open reference source make bounded start/stop lifecycles the safest
design.

## Uncertainty

Google publishes Quick Share user behaviour and Nearby Connections APIs, but no
authoritative specification maps Quick Share visibility to BLE, Bluetooth
Classic, LAN, hotspot, or Wi-Fi Direct calls. The recommendation therefore uses
documented Quick Share visibility, documented Nearby and radio lifecycle costs,
and an explicit implementation inference. It does not claim to reproduce the
stock product's scan cadence, radio ordering, or background policy.
