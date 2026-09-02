# Existing solutions for the Android test lab

Research cutoff: 2026-09-02

## Decision

Keep the two AVDs on the same Linux host as an experimental control. Mobly
1.13.1 is the two-device runner, while the project's small host launcher owns
AVD creation and lifecycle. The launcher has its own ADB server, private key,
emulator home, AVD home, and fixed device serials.

Reuse two proven designs rather than adopting their whole distributions:

- Google's Android Emulator Container Scripts provide the recovery pattern:
  install one known ADB key, use `-skip-adb-auth`, wipe untrusted user data,
  disable snapshot saving, and wait for boot completion.
- Android's BeToCQ suite provides the Nearby Connections test pattern:
  assign explicit peer roles, drive both through Mobly snippets, wait for
  callbacks, verify the selected route where possible, and clean up every
  endpoint.

Do not use Gradle Managed Devices or separate emulator containers as the
pair coordinator. Both manage individual devices well. Neither documents a
shared control channel and radio fabric for one coordinated two-peer test.

No maintained library exposes stock Quick Share as a test API. A public
Nearby Connections probe can test the lower Connections layer. Stock Quick
Share still needs UI Automator and must remain a pinned Google AVD check, not
a claim about physical or OEM devices.

## Fit summary

| Candidate                          | Maintained state                                  | Useful part                                                     | Decision                                    |
| ---------------------------------- | ------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------- |
| Android Emulator Container Scripts | Active at `0654f694`, 2026-07-24; experimental    | Headless ADB key injection, wipe, fixed ports, boot probe       | Reuse the launch pattern, not Docker        |
| Gradle Managed Devices             | Current Android Gradle Plugin feature             | Create, snapshot, restore, test, and tear down one or many AVDs | Do not use for peer coordination            |
| Mobly 1.13.1                       | Released 2026-08-07; repository active 2026-08-12 | Named Android controllers, two-device barriers, callbacks, logs | Adopt for Android peer tests                |
| Mobly Bundled Snippets             | Repository active 2026-06-10                      | Maintained example of device-side RPC and UI Automator          | Build a smaller project-owned snippet       |
| BeToCQ 3.1.1                       | Released 2026-08-28                               | First-party two-device Nearby test design and route metrics     | Reuse its design; do not run the full suite |
| Public Nearby Connections 19.5.0   | Published 2026-08-24                              | Advertising, discovery, consent, payloads, cancellation         | Retain as an experimental control           |

## ADB authorization and isolation

ADB's current host code always loads or creates the private key at
`$HOME/.android/adbkey`, then adds keys from `ADB_VENDOR_KEYS`. The latter is
additive. It cannot stop a personal default key from being tried first.
[ADB host authentication source](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/client/auth.cpp)

The ADB command line supports a separate server socket through `-L`, a
separate client port through `-P`, a foreground `server nodaemon` process, and
`keygen FILE`. `ADB_SERVER_SOCKET` takes precedence over the older server host
and port variables when the command has no explicit server option.
[ADB manual](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/docs/user/adb.1.md),
[server option parsing](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/client/commandline.cpp)

The lab must therefore start the pinned Platform-Tools binary with a
process-local home and server port. Every launcher, ADB client, and Mobly
process must inherit the same values:

```text
HOME=<cache>/android/adb-home
ANDROID_EMULATOR_HOME=<cache>/android/adb-home/.android
ANDROID_AVD_HOME=<cache>/android/avd
ANDROID_ADB_SERVER_PORT=5038
ADB_VENDOR_KEYS=<cache>/android/adb-home/.android/adbkey
```

Generate that key once with `adb keygen`, bind the lab server to loopback,
run it as a supervised `server nodaemon` process, and stop only that server.
Always pass a serial for device commands. Android documents the emulator home
and AVD home overrides; the ADB source itself uses the process home for its
default key.
[Android tool environment variables](https://developer.android.com/tools/variables),
[ADB home resolution](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/adb_utils.cpp)

An `unauthorized` transport means ADB exhausted its known private keys and
sent a public key for user approval. A headless device cannot click that
dialog. Restarting the same server does not create trust.
[ADB connection states](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/adb.h),
[ADB authorization flow](https://android.googlesource.com/platform/packages/modules/adb/+/HEAD/docs/dev/adbd_framework.md),
[Android RSA prompt](https://developer.android.com/tools/adb#Enabling)

The emulator has a test-only `-skip-adb-auth` option. Google's maintained
container launcher combines it with a supplied private key, `-wipe-data`,
`-no-snapshot-save`, fixed ports, and `-no-window`. This is a better authority
than folklore about deleting random files from an AVD.
[emulator option source](https://android.googlesource.com/platform/external/qemu-android/+/ec81994cc237758ab4313e9f808a158da5640ada/qemu-options.hx),
[Google container launcher](https://github.com/google/android-emulator-container-scripts/blob/0654f694b46794fae4b178f1e1a17cb60c5d2d34/emu/templates/launch-emulator.sh#L54-L78),
[launch flags](https://github.com/google/android-emulator-container-scripts/blob/0654f694b46794fae4b178f1e1a17cb60c5d2d34/emu/templates/launch-emulator.sh#L169-L180)

Use `-skip-adb-auth` only for lab-owned AVDs whose ADB ports remain on
loopback. It removes a security boundary.

### Local result

The minimized API 36 Google Play setup reached `unauthorized` in about 17
seconds. A dedicated server and shared lab key did not repair the existing
user data. Adding `-wipe-data`, while using the lab-owned home, emulator home,
server port, and pinned `-adb-path`, changed the observed state from `offline`
to `device`.

That experiment is consistent with Google's emulator infrastructure, which
places the host public key into a public image during first launch. It also
shows why the key, AVD user data, and baseline snapshot form one identity.
[Google emulator image key setup](https://android.googlesource.com/platform/external/adt-infra/+/eb14c631b6dd60c59fe0b41c16007817ea28b940/emu-image/README.md)

The recovery sequence should be bounded:

1. Try `adb reconnect offline` once.
2. Stop the AVD and boot once with `-wipe-data`.
3. Delete and recreate that lab-owned AVD once.
4. Fail with emulator output, ADB state, and logcat retained.

Never loop on `unauthorized`. Once a clean boot passes, create the named test
snapshot. Rotate the key, wipe, or recreate whenever the image, emulator,
device definition, or lab key changes.

The emulator documents that `-wipe-data` restores the initial user image and
removes installed apps and settings. `avdmanager delete avd` and `create avd`
provide the full replacement path.
[wipe behavior](https://developer.android.com/studio/run/emulator-commandline#data-filedir),
[`avdmanager` commands](https://developer.android.com/tools/avdmanager)

## Why not adopt the container scripts

The Google scripts are active and Apache-2.0, but the README still calls them
experimental. They require Linux, Python 3.10, ADB, Docker Engine, Docker
Compose, KVM, and full emulator and system-image layers. The hosted registry
still lists an API 28 Play Store image and API 29 or 30 Google images on
emulator 30.1.2. This lab needs the pinned API 36 image and emulator 37.1.11.
[container requirements](https://github.com/google/android-emulator-container-scripts/blob/0654f694b46794fae4b178f1e1a17cb60c5d2d34/README.md),
[hosted image registry](https://github.com/google/android-emulator-container-scripts/blob/0654f694b46794fae4b178f1e1a17cb60c5d2d34/REGISTRY.MD)

The template launches one fixed `MediumPhone` AVD, one internal ADB server,
and one set of ports. Google does not document a shared Netsim radio fabric
between two containers. A Docker bridge supplies IP connectivity, but that
does not make BLE or Wi-Fi Direct cross the container boundary. This is an
inference from the launcher and the missing multi-container topology.

Use its key and lifecycle sequence on the host. Docker would add more setup,
slower image preparation, and another network boundary without solving the
two-radio problem.

The scripts are Apache-2.0. The downloaded SDK and Google Play image remain
under the Android SDK License and must stay in the ignored test cache.
[container license notice](https://github.com/google/android-emulator-container-scripts/blob/0654f694b46794fae4b178f1e1a17cb60c5d2d34/emu/templates/emulator.README.MD#license)

## Why Gradle Managed Devices is not the coordinator

Gradle Managed Devices creates, starts, snapshots, restores, tests, and tears
down virtual devices. It restores a clean state between instrumented tests.
Setting `systemImageSource = "google"` selects a Google Play system image in
the Android Gradle Plugin source.
[managed-device guide](https://developer.android.com/studio/test/managed-devices),
[image mapping](https://android.googlesource.com/platform/tools/base/+/ccd6b7a1bc0b6ef38c11ad9594d0d49905ac930c/build-system/gradle-core/src/main/java/com/android/build/gradle/internal/tasks/ManagedDeviceSetupTask.kt)

A device group runs the test suite independently on every device, usually in
parallel. The documented API has no sender and receiver roles, cross-device
barrier, or host callback channel. It is a lifecycle manager, not a peer-test
runner. Adding a rendezvous service around two independent instrumentations
would recreate the part Mobly already supplies.

GMD is useful for ordinary one-device probe tests. It should not own the
Quick Share pair or its seeded Google Play snapshot.

## Adopt Mobly for the pair

Mobly 1.13.1 is Apache-2.0, requires Python 3.11 or later, and depends mainly
on PyYAML and `portpicker`. Its wheel is about 161 KB. It expects ADB and the
devices to exist already.
[Mobly 1.13.1](https://github.com/google/mobly/releases/tag/1.13.1),
[package definition](https://github.com/google/mobly/blob/1.13.1/pyproject.toml)

Its official tutorial configures two serials with labels, requires
`min_number=2`, and selects each role by label. The Android controller verifies
that configured serials are reachable, creates an ADB proxy for each device,
starts per-device services, and stops them during teardown. It does not create
or wipe AVDs.
[two-device tutorial](https://github.com/google/mobly/blob/1.13.1/docs/tutorial.md#example-5-test-with-multiple-android-devices),
[Android controller](https://github.com/google/mobly/blob/1.13.1/mobly/controllers/android_device.py)

This division is clean:

- the host launcher owns SDK pins, ADB, AVD processes, snapshots, and cleanup;
- Mobly owns roles, test ordering, callbacks, assertions, and artifacts;
- one small snippet APK exposes only the Android operations the tests need.

Mobly Snippet Lib turns annotated device methods into host RPC calls over an
ADB-forwarded socket and supports asynchronous callbacks. UI Automator can
drive other apps and system UI. The latest Snippet Lib release is still 1.4.0
from 2023, but the Google Mobly Bundled Snippets repository was active in June
2026 and still uses it. Pin both if this route is adopted.
[Snippet Lib 1.4.0](https://github.com/google/mobly-snippet-lib/tree/1.4.0),
[maintained bundled-snippet use](https://github.com/google/mobly-bundled-snippets/blob/d3d38b5faee50fbdd90dc5ae851a08214be07857/build.gradle#L73-L83)

Do not install the broad bundled snippets APK merely for one or two RPCs. Its
build pulls UI Automator, Gson, Guava, Kotlin, and other test dependencies.
Keep these dependencies in the prepared Android test environment, never in
the Rust application or user-facing plugin.

## Nearby Connections and Quick Share

The current public Android dependency is
`com.google.android.gms:play-services-nearby:19.5.0`. Its AAR is about 1.4 MB
before transitive AndroidX, Play services, Kotlin, and coroutines libraries.
Its POM names the Android SDK License.
[Google Maven metadata](https://dl.google.com/dl/android/maven2/com/google/android/gms/play-services-nearby/maven-metadata.xml),
[19.5.0 POM](https://dl.google.com/dl/android/maven2/com/google/android/gms/play-services-nearby/19.5.0/play-services-nearby-19.5.0.pom)

A probe on each AVD can exercise advertising, discovery, connection request,
accept, reject, byte or file payloads, cancellation, disconnect, and cleanup.
Google's API requires both peers to use the same strategy and service ID. A
payload is complete only when its transfer callback reports success.
[advertise and discover](https://developers.google.com/nearby/connections/android/discover-devices),
[connection management](https://developers.google.com/nearby/connections/android/manage-connections),
[payload exchange](https://developers.google.com/nearby/connections/android/exchange-data),
[`ConnectionsClient`](https://developers.google.com/android/reference/com/google/android/gms/nearby/connection/ConnectionsClient)

The public 19.5.0 builders expose strategy, low-power mode, and connection
type. They do not expose BeToCQ's setters for exact advertising, discovery,
connection, or upgrade media. BeToCQ builds against additional Google APIs
such as `Medium`, `setAdvertisingMediums`, and `setUpgradeMediums` that are
absent from the public AAR. Its released APKs may provide those controls, but
the open source cannot reproduce them from the public Maven dependency.
[BeToCQ medium factory](https://github.com/android/betocq/blob/3.1.1/betocq/app/src/main/com/google/android/nearby/mobly/snippet/connection/MediumSettingsFactory.java),
[public options reference](https://developers.google.com/android/reference/com/google/android/gms/nearby/connection/AdvertisingOptions)

BeToCQ 3.1.1 is still the best first-party design reference. It assigns two
Mobly device roles and records discovery, connection, upgrade, transfer, and
failure metrics. It is not a fast virtual test dependency. Its 38.2 MB wheel
bundles three roughly 13 MB snippet APKs and requires Python 3.12, Mobly,
Mobly Wi-Fi, partner tools, two userdebug physical devices, configured access
points, and preferably RF isolation. Google states that the suite takes two
to six hours.
[BeToCQ 3.1.1 release](https://github.com/android/betocq/releases/tag/3.1.1),
[requirements and duration](https://github.com/android/betocq/blob/3.1.1/README.md),
[package dependencies](https://github.com/android/betocq/blob/3.1.1/build_config/nearby_connection/pyproject.toml),
[two-peer wrapper](https://github.com/android/betocq/blob/3.1.1/betocq/nearby_connection/nearby_connection_wrapper.py)

Also, BeToCQ's "Quick Start" target tests Android device onboarding. It is
not Google Quick Share.

Google publishes no API for starting, accepting, cancelling, or reading the
result of stock Quick Share. The public Nearby API tests the Connections
layer, not Quick Share's Sharing messages and attachment semantics. Google's
open Nearby repository confirms that Quick Share adds its own packet format.
[Quick Share distinction](https://github.com/google/nearby/discussions/2446),
[open Nearby scope and license](https://github.com/google/nearby)

For the stock-product control, keep the existing plan: launch
`ACTION_SEND` or `ACTION_SEND_MULTIPLE`, select Quick Share, and drive both
system UIs with UI Automator. UI Automator is intended for cross-app and
system-UI tests, but its selectors and synchronization are less stable than a
public API. Save screenshots and UI hierarchies on failure.
[UI Automator](https://developer.android.com/training/testing/other-components/ui-automator),
[instrumented-test stability](https://developer.android.com/training/testing/instrumented-tests/stability)

## Two virtual peers and the remaining proof gap

Emulator 36.5 and later puts instances on one shared virtual Wi-Fi network and
documents NSD and Wi-Fi Direct between them. The capability table lists
Bluetooth Classic and BLE on API 31 and later. Emulator 37.1.11 is above that
threshold.
[multi-emulator interconnection](https://developer.android.com/studio/run/emulator-networking-interconnect),
[radio capability table](https://developer.android.com/studio/run/emulator-networking)

That makes two host AVDs a credible control pair. It does not make every
medium controllable through the public Nearby API. Use Netsim capture and
radio availability to observe which route ran, and keep direct BLE, Classic,
NSD, and Wi-Fi Direct probe tests as separate emulator capability checks.

### Local admission result

The pinned control was tried on API 36 revision 7 with both the Google Play
and Google APIs x86_64 variants. The Google APIs variant is the appropriate
choice for the probe because Android documents that it includes Google Play
services without requiring the Play Store image. Its seeded two-peer warm
startup reached ready in 50.9 seconds on the reference host, compared with
114 to 230 seconds for observed Play Store runs.
[AVD image variants](https://developer.android.com/studio/run/managing-avds#system-image),
[Play services emulator setup](https://developers.google.com/android/guides/setup)

Neither image passed the Nearby admission control. On both peers, Wi-Fi,
Bluetooth, and location were enabled and all declared runtime permissions
were granted. The snippet RPC control responded, but every independent test
left `startAdvertising` pending until its deterministic timeout. The Play
Store run also recorded Google Play services disconnections. No discovery,
connection, or payload assertion was reached.

The AVD route is therefore unsupported as of the research cutoff. Keep its
provisioner, diagnostics, and explicit test target so a pinned emulator,
system-image, or Play services update can be evaluated. Do not put it in the
authoritative gate or cite it as Android compatibility evidence until an
AVD-to-AVD control passes repeatably. This does not block implementation
against the admitted C++ oracle, medium simulator, virtual services, and
virtual-radio routes.

The resulting claims must remain narrow:

- A two-AVD Nearby pass proves public Nearby Connections worked on the route
  Google Play services chose.
- A two-AVD stock pass proves that one pinned Google image sent and received
  through its current Quick Share UI.
- Neither proves Linux interoperability, a specific medium, OEM behavior, or
  physical-radio compatibility. Those need the independent Google protocol
  oracle and Linux virtual-radio tests already defined elsewhere.
