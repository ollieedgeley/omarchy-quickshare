# Live bandwidth-upgrade topology research

Research date: 2026-09-02

## Question

Can two Google-derived `file_share` processes start over a real virtual
Bluetooth Classic connection and then upgrade over LAN without exposing host
Bluetooth controllers to Docker peers?

## Result

Do not use a Docker network namespace as the boundary for an H4-attached
controller. The probe described below showed that the host Bluetooth stack
still observed the controller created by `btattach`. Use a KVM guest as the
controller-ownership boundary instead.

This is a topology recommendation, not a live bandwidth-upgrade gate. The
repository has no such gate today.

## Evidence from the pinned sources

The pinned BlueZ source is commit
[`3a2d543c4c21d9c1dab246d46d76a11996a69bf2`](https://github.com/bluez/bluez/tree/3a2d543c4c21d9c1dab246d46d76a11996a69bf2).
Its `btvirt -l2` mode opens two virtual HCI controllers through `/dev/vhci`.
The repository's radio guest uses that exact command, waits for two `hci*`
devices, and starts BlueZ against them. [BlueZ `btvirt` option handling](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/emulator/main.c#L44-L184), [project radio guest](../../tests/environments/bluez/radio-guest-init.sh)

`btvirt -s` instead opens Unix-domain H4 server sockets. Each accepted
connection gets its own `btdev`; the emulator keeps its `btdev` registry in
the same process, which is the property needed for two virtual controllers to
discover and connect to one another. [Server setup](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/emulator/main.c#L185-L226), [one `btdev` per client](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/emulator/server.c#L253-L332), [global device lookup](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/emulator/btdev.c#L303-L367)

`btattach` does not merely proxy packets in user space. It puts a TTY into the
`N_HCI` line discipline, selects an HCI UART protocol, and asks the kernel for
the resulting HCI device index. [BlueZ `btattach`](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/tools/btattach.c#L40-L130)
The Linux source names HCI UART and its H4 protocol as kernel support for
serial Bluetooth controllers. [Linux HCI UART configuration](https://github.com/torvalds/linux/blob/v6.1/drivers/bluetooth/Kconfig#L36-L59)

The present Nearby Linux peer does not contain a real controller. Its entry
script starts a D-Bus BlueZ mock, adds a mock `hci0`, and then launches
NetworkManager and Avahi. That is enough for the LAN tests, but not for a
Classic connection. [Peer entry script](../../tests/environments/nearby-linux/assets/peer-entrypoint.sh)

## Docker H4 shortcut rejected

The tempting arrangement is a `btvirt -s` sidecar, one Unix socket client per
Docker peer, a pseudo-terminal bridge, and `btattach` in each peer. It would
look like this:

```text
peer A: bluetoothd + HCI UART  <--- H4 --->  btvirt
peer B: bluetoothd + HCI UART  <--- H4 --->  btvirt
peer A eth0                    <--- LAN ---> peer B eth0
```

That is not an acceptable isolation claim. The parent probe attached an H4
controller inside a network namespace. `btattach` reported that it created
`hci1`, but host `btmgmt` then saw both `hci0` and `hci1`. The namespace query
saw no controller and hung. Cleanup restored the host to `hci0` only.

The observation is narrow but decisive for this environment: an HCI device
created this way is not confined well enough by the Docker network namespace.
Do not add the sidecar, `socat`, or container capabilities on the premise that
each peer owns its controller. Sharing `/dev/vhci` would make ownership less
clear, not more clear.

## Recommended KVM topology

Keep the Bluetooth controllers inside one disposable KVM guest. The existing
radio environment already supplies the important part: a guest-local
`btvirt -l2` process and two guest-local HCI controllers.

```text
host
  QEMU/KVM guest
    btvirt -l2
      hci0 <-> hci1
    peer A namespace: BlueZ bound to hci0, file_share A
    peer B namespace: BlueZ bound to hci1, file_share B
    veth pair or guest bridge between the peer namespaces for LAN
```

Run a private system D-Bus and a BlueZ daemon bound to the assigned controller
for each peer. Give each peer its own network namespace and address on the
guest LAN. Run its NetworkManager and Avahi on the matching private bus, then
start the Google-derived `file_share` process with the matching system-bus
address. The guest owns every controller and can discard them at shutdown.
The host and Docker peers never receive `/dev/vhci`, an HCI UART attachment, or
an HCI device.

This arrangement needs a small guest image or a 9p-mounted test root containing
the prepared Nearby binaries and peer services. It is more work than the
rejected Docker shortcut, but it has a boundary that the probe did not break.

Do not split the current `btvirt -l2` radio across two independent guests.
The emulator's radio state and its device registry live in one process. Two
separate `btvirt` processes make two separate emulated radio worlds. A
two-guest design would therefore need a separately designed HCI/radio bridge.

## What a future experiment must show

Before a test can claim a live upgrade, it needs all of the following:

- Classic discovery and an encrypted initial channel on the assigned
  controllers in both transfer directions.
- A `bandwidth_changed` event to LAN from both Google-derived peers.
- A byte-integrity check after the migration, not merely before it.
- Cleanup that removes the guest and leaves no host HCI controller, D-Bus
  service, socket, or namespace.

Until then, the existing Nearby Linux Connections test proves a live LAN
transfer only.

## Attempt closed on 2026-09-03

Two bounded reference-peer experiments tested the remaining path. Neither
qualified as a gate, so their disposable harnesses were removed instead of
becoming unsupported project tooling.

The BLE-to-LAN experiment ran two KVM guests with separate BlueZ instances and
controllers connected through Bumble. The advertising guest emitted the
expected FEF3 legacy service data. The scanning guest received the HCI
advertising report, BlueZ created `Device1`, exposed `ServiceData`, and
activated the advertisement monitor. BlueZ did not call the Google-derived
monitor's `DeviceFound` callback. The peer therefore produced no endpoint-found
event, connection, upgrade, or payload result. A test-only pattern change from
the UUID bytes to the observed payload header did not change that result and
was discarded.

The Classic-to-LAN experiment also ran two isolated KVM guests. Sequential
controller startup removed a race in parallel HCI initialization, and the
advertising peer reported successful Classic advertising. The discovering peer
then timed out before reporting discovery. BlueZ logged a rejected privacy
setting on that controller. No encrypted initial connection, bandwidth change,
or payload transfer occurred.

Both test phases completed within their gate budget and cleaned their guests,
controllers, sockets, and runtime files. The failures sit inside the pinned
Linux reference stack before a bandwidth upgrade can begin. They do not weaken
the admitted virtual BLE, Classic, LAN, hotspot, Wi-Fi Direct, simulated
upgrade, fixture, or live Sharing checks.

Application work is not blocked by these rows. The development start condition
uses the admitted fixtures, shared contracts, radio and network self-tests, and
fault injectors. A future retry should start only when a source change or new
reference peer can demonstrate the missing discovery callback. It must still
meet every live-upgrade requirement above before joining `make verify`.

## KVM sidecar probe update

The first KVM-sidecar probe initially reported a missing controller because
the guest waited for `/sys/class/bluetooth/hci0/address`; that attribute is not
provided by this guest kernel. A one-peer boot instead shows `btattach` H4
traffic reaching the isolated `btvirt` sidecar, then guest `btmgmt` and
`hciconfig` reporting one UART controller with address
`00:AA:01:00:00:42`. Controller attachment therefore works without relaxing
the QEMU container's seccomp profile.

The probe now starts guest control independently of controller bring-up, then
uses a bounded `hciconfig hci0 up` command. A sidecar H4 trace showed that the
Classic failure was not inquiry, page, or RFCOMM listener setup: both guests
completed an ACL connection, after which a split H4 ACL frame reached `btvirt`
as an invalid packet. An unprivileged relay in the QEMU runner now reassembles
complete H4 frames before forwarding them to the sidecar. The raw two-guest
RFCOMM byte roundtrip and the controller, LAN, and Classic self-test pass with
the QEMU runner on its default seccomp profile. The relay adds no network,
KVM, device, or mount access; the isolated non-root, read-only `btvirt`
sidecar remains the only container with relaxed seccomp.
