# Android AVD verification for Quick Share

Research cutoff: 2026-09-02

## Verdict

Use two API 36 Google Play AVDs as a black-box control pair, plus a small
Nearby Connections probe APK. Do not treat either one as proof that a Linux
Quick Share endpoint works.

The emulator can now model the radios that matter. Emulator 36.5 and later
puts instances on one virtual Wi-Fi network and documents NSD and Wi-Fi
Direct between them. The current capability table also lists Bluetooth
Classic and BLE for API 31 and later. Emulator 37.1.11 is the current stable
release. These facts make an AVD-to-AVD control worth building, but Google
does not document Quick Share as an AVD feature or publish a Quick Share test
API. [Interconnection guide](https://developer.android.com/studio/run/emulator-networking-interconnect),
[capability table](https://developer.android.com/studio/run/emulator-networking),
[37.1.11 release notes](https://developer.android.com/studio/releases/emulator#37-1-11)

The selected Google Play image is necessary but not sufficient. Android's AVD
guide says a Google Play phone image includes the Play Store and Google Play
services. Google says Quick Share is a Google Play services device-connection
feature. Neither source promises that the API 36 emulator build contains a
working Quick Share UI. Stock Quick Share on this image is therefore
unproven until the image boots and completes the control test described below.
[AVD image distinction](https://developer.android.com/studio/run/managing-avds#system-images),
[Play services device connections](https://support.google.com/android/answer/13133588)

There is also no supported way to install a separately versioned stock Quick
Share APK. Seed the image through its own Google Play services update path,
record what Google supplied, and save a local snapshot. If the UI is absent or
the two AVDs cannot share in both directions, the stock-product route is
unsupported for this image. A Nearby Connections probe remains useful, but it
must not replace that failed test.

## Exact Linux x86-64 baseline

The following stable-channel packages are the reproducible baseline. The
official repository metadata supplied the package revisions, archive names,
sizes, and SHA-1 values on the research cutoff. The SHA-256 values below were
calculated directly from the named Google archives.
[SDK repository metadata](https://dl.google.com/android/repository/repository2-3.xml),
[Google Play image metadata](https://dl.google.com/android/repository/sys-img/google_apis_playstore/sys-img2-3.xml)

| Package            | Pin                                                                | Linux archive                                | Google SHA-1                               | Verified SHA-256                                                   |
| ------------------ | ------------------------------------------------------------------ | -------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------ |
| Command-line tools | `cmdline-tools;23.0`                                               | `commandlinetools-linux-16111833_latest.zip` | `e025545c62a8e64c7559119566a569fb1dec5f60` | `0877a1d048fe4a24efe2eff536ca4223f7adeb58648bb81909d33c446918cfa8` |
| Platform tools     | `platform-tools` 37.0.1                                            | `platform-tools_r37.0.1-linux.zip`           | `477254aa5f903c15cf51001717bdf347fb6b53e0` | `d230f13842f60f782a8645f9c813f8f845bf36089ea7289f28c48f17979313f1` |
| Emulator           | `emulator` 37.1.11, stable                                         | `emulator-linux_x64-15917651.zip`            | `1b1f78891abf8ec268264356e1365c25519e8379` | `95771e0ae431897b2a4bd2d97fa095f29a8b0624a7b216baf529f9306161c266` |
| Android platform   | `platforms;android-36` revision 2                                  | `platform-36_r02.zip`                        | `2c1a80dd4d9f7d0e6dd336ec603d9b5c55a6f576` | `37607369a28c5b640b3a7998868d45898ebcb777565a0e85f9acf36f29631d2e` |
| Build tools        | `build-tools;36.1.0`                                               | `build-tools_r36.1_linux.zip`                | `936a0d6bd5ae3e2118a7567dddbc95ff67ed46e9` | `a7b5889e4a79fcf3b0976bef40d401f4240fb1eed891d9d91169da1111e11d78` |
| Google Play image  | `system-images;android-36;google_apis_playstore;x86_64` revision 7 | `x86_64-36_r07.zip`                          | `16fa3c441d29fde6c9eea2f766eecf77032d68b4` | `279c419c2829627e2bb95104f47e6de02ca9d14774506195d5db93d71bd583c8` |

The test-only probe build also pins its host toolchain. Use Temurin
21.0.12.1+1 from
`OpenJDK21U-jdk_x64_linux_hotspot_21.0.12.1_1.tar.gz` with SHA-256
`ce79869e1307ed8ee1e2baa86a412b1eb5b75d10a01006d788a6f968bcfaee94`,
and Gradle 9.1.0 from `gradle-9.1.0-bin.zip` with SHA-256
`a17ddd85a26b6a7f5ddb71ff8b05fc5104c0202c6e64782429790c933686c806`.
The manifest records their origins and sizes as well as these digests.
[Gradle checksum](https://services.gradle.org/distributions/gradle-9.1.0-bin.zip.sha256),
[Temurin checksum](https://github.com/adoptium/temurin21-binaries/releases/download/jdk-21.0.12.1%2B1/OpenJDK21U-jdk_x64_linux_hotspot_21.0.12.1_1.tar.gz.sha256.txt)

The metadata also lists emulator 37.2.7 on development channel 2. It uses the
preview license and is not this baseline. An unqualified `latest` dependency
would therefore be wrong. Google's tooling documentation tells scripts to
select explicit versions.
[Android CLI package versions](https://developer.android.com/tools/agents/android-cli#sdk-install)

API 36 is deliberate. Emulator 36.6.11 raised API 37 phone AVDs to a strict
minimum of 4 GiB each. Two API 37 peers would reserve at least 8 GiB before the
host test process and Linux reference peer. The current development machine
has 7.6 GiB of physical RAM, so API 37 cannot meet the project's fast feedback
target here. [API 37 memory change](https://developer.android.com/studio/releases/emulator#36-6-11)

The lock manifest should keep the package path, revision, archive URL, archive
size, official SHA-1, locally verified SHA-256, and license identifier. It
should reject a different installed `Pkg.Revision`, even if Google's package
installer successfully installs it. Google may replace the stable catalog
entry later.

## License boundary

Every listed package refers to Google's Android SDK license. The user who will
run the SDK must accept it. Command-line Tools 23.0 changed the mechanism:
`sdkmanager` is now a compatibility wrapper that delegates to `android sdk`,
and `sdkmanager --licenses` exits after warning that the option is no longer
needed. A successful wrapper call is not evidence of acceptance. Provisioning
must use the interactive Android CLI install flow and verify the resulting
license records. The repository must not answer for the user.
[wrapper source](https://android.googlesource.com/platform/tools/base/+/dcded801a4942d06805cc74b56dc2cecbece2f80/sdklib/wrappers/sdkmanager.sh),
[Android CLI license compatibility](https://developer.android.com/tools/agents/android-cli/release-notes)

The license requires acceptance before SDK use. It also prohibits copying,
modifying, or redistributing closed SDK components except for backup or where
an open-source component license permits it. Keep the SDK, AVD data, Google
Play image, updated Google components, and snapshots in an ignored local
cache. Do not put them in Git, a release archive, or the Omarchy plugin.
[Android SDK License sections 2 and 3](https://developer.android.com/studio/terms#2-accepting-this-license-agreement)

The test probe can still be installed with `adb`. Google's current developer
verification FAQ explicitly exempts ADB-installed development and test apps
from registration. [ADB verification exemption](https://developer.android.com/developer-verification/guides/faq#adb)

## Host and emulator requirements

Use Linux x86-64 with KVM. The emulator command documentation says accelerated
x86 and x86-64 emulation uses KVM on Linux. Run `emulator -accel-check` and
make an unusable `/dev/kvm` a provisioning failure. Software CPU translation
is too slow for the lifecycle budget.
[emulator acceleration](https://developer.android.com/studio/run/emulator-acceleration#check-hypervisor),
[command-line acceleration](https://developer.android.com/studio/run/emulator-commandline#accel)

The current host has `/dev/kvm`, and the repository user can open it. It has
7.6 GiB RAM and 15 GiB swap. Start no more than two API 36 AVDs at once, and
measure both warm startup and shutdown. Do not hide swapping behind the test
timer.

Start each peer with a distinct even console port. Use `-accel on`,
`-no-window`, `-no-audio`, `-no-boot-anim`, and `-gpu auto`. Give both
processes the same isolated `ANDROID_TMP` so their Netsim artifacts and radio
captures stay together. The emulator documents headless mode, no-audio mode,
boot-animation suppression, fixed ports, and packet capture.
[emulator command line](https://developer.android.com/studio/run/emulator-commandline)

Prepare a named snapshot only after the control pair is healthy. Start tests
from that snapshot with snapshot saving disabled, so one test cannot alter the
next test's baseline. Android documents that a snapshot contains OS settings,
application state, and user data. It also warns that emulator, system-image,
and AVD-configuration changes invalidate snapshots.
[snapshot behavior](https://developer.android.com/studio/run/emulator-snapshots)

## Provisioning contract

These command shapes are the contract for the future environment tool. Paths
and device names belong in its lock manifest rather than being scattered
through hooks.

```bash
android sdk install \
  platform-tools@37.0.1 \
  emulator@37.1.11 \
  platforms/android-36@2 \
  build-tools/36.1.0 \
  system-images/android-36/google_apis_playstore/x86_64@7

printf 'no\n' | avdmanager create avd \
  --name quickshare-a \
  --package "system-images;android-36;google_apis_playstore;x86_64" \
  --device pixel_6

printf 'no\n' | avdmanager create avd \
  --name quickshare-b \
  --package "system-images;android-36;google_apis_playstore;x86_64" \
  --device pixel_6
```

`avdmanager` documents the package and device arguments. The environment tool
must first confirm that `pixel_6` occurs in `avdmanager list device`; a renamed
or missing profile is an error, not permission to pick another profile.
[avdmanager reference](https://developer.android.com/tools/avdmanager)

The first seed boot is an explicit networked provisioning operation. It should:

1. Cold boot both AVDs and wait for `sys.boot_completed=1` and a responsive
   package manager.
2. Set an English locale, deterministic device names, and fixed display size.
3. Record each build fingerprint and the version code and version name of
   `com.google.android.gms`.
4. Use the Google Play update control to bring Play services to the chosen
   state. The AVD guide documents that update control for Google Play phone
   images.
5. Confirm that Settings exposes Quick Share and that the share sheet contains
   a Quick Share target. Google publishes no stable activity name, resource ID,
   or public `Settings` intent for this UI, so discover it by visible semantics.
6. Turn on Bluetooth and Wi-Fi, open Quick Share receive mode, and select the
   documented "Everyone for 10 minutes" visibility for each receiver.
7. Pass the stock control matrix below, then save the local snapshot and its
   provenance record.

Google's user documentation requires Bluetooth, an unlocked receiving screen,
receive mode, and explicit accept or decline. It also documents sending files
and webpages through the Android share sheet and the temporary everyone
visibility mode. [Quick Share user flow](https://support.google.com/android/answer/15728591)

Use UI Automator through an instrumentation APK for the closed UI. UI Automator
is designed for cross-application and system-UI tests and can take screenshots
and inspect visible elements. Do not bind the driver to an undocumented Google
resource ID without also retaining a text or accessibility fallback.
[UI Automator scope](https://developer.android.com/training/testing/other-components/ui-automator)

Install the probe and UI driver with `adb install -r`. A small host runner can
use Mobly 1.13.1 and Mobly Snippet Lib 1.4.0, pinned by source commit. Mobly can
control several ADB devices and collect artifacts; Snippet Lib exposes test APK
methods to the host and includes a UI Automator example. Both projects state
that they are Google-developed, not official Google products.
[Mobly 1.13.1](https://github.com/google/mobly/releases/tag/1.13.1),
[Mobly](https://github.com/google/mobly),
[Snippet Lib](https://github.com/google/mobly-snippet-lib/tree/1.4.0)

## Required self-tests

### Radio controls before Quick Share

Before looking at the product UI, prove that the pair can communicate through
the emulator facilities:

- Use a tiny NSD test app to publish and discover in both directions.
- Use Android's Wi-Fi Direct API in a test app to form a group with each AVD in
  the supported roles, then exchange bytes both ways.
- Use Android Bluetooth test apps to exercise BLE advertising, scanning, GATT,
  and Bluetooth Classic connections between the two AVDs.
- Enable Netsim packet capture and retain one capture per radio on failure.
- Change BLE and Classic RSSI through `-netsim-args`, observe the effect, then
  restore the control path.

The emulator documents shared NSD and Wi-Fi Direct for version 36.5 and later.
Its advanced networking interface documents per-radio packet capture and RSSI
control for BLE and Bluetooth Classic.
[multi-AVD networking](https://developer.android.com/studio/run/emulator-networking-interconnect),
[Netsim controls](https://developer.android.com/studio/run/emulator-networking-advanced)

### Public Nearby Connections control

Install the same probe on both AVDs. Run advertiser and discoverer in both
directions. Each run must cover request, mutual accept, reject, file send,
receiver file SHA-256, sender cancellation, receiver cancellation, disconnect,
and a second clean connection.

The public API supports advertising, discovery, request, accept, reject,
payload send, payload cancellation, disconnect, and stop-all. Its completion
callback reports whether a file transfer succeeded or failed. It does not
offer a documented way to force or report the selected medium. The result is
therefore "Google Play services Nearby Connections works on a route it chose,"
not "Quick Share works" and not "every medium works."
[ConnectionsClient](https://developers.google.com/android/reference/com/google/android/gms/nearby/connection/ConnectionsClient),
[file payload completion](https://developers.google.com/nearby/connections/android/exchange-data)

### Stock Quick Share control

Run every case A to B and B to A:

- one binary file, verified by size and SHA-256 on the receiver;
- multiple files, preserving attachment count and individual hashes;
- plain text, verified from the receiver's observable UI or destination;
- one HTTP URL, verified as the exact received URL;
- receiver accept and decline;
- sender cancellation and receiver cancellation during transfer;
- a second clean transfer after each terminal result.

Drive sending through `ACTION_SEND` or `ACTION_SEND_MULTIPLE`, then choose the
visible Quick Share target. Drive receive mode, peer selection, confirmation,
and cancellation through UI Automator. Save the UI hierarchy, screenshots,
logcat, build fingerprints, Play services versions, Netsim captures, and
payload hashes on failure. Never save peer display names, account data, source
filenames, keys, or payload contents in a committed artifact.

Passing this matrix proves only that one pinned Google image can send to and
receive from the same pinned Google image under emulation. It does not prove
Linux interoperability.

### Android against a Linux reference peer

The next self-test must use a pinned Google-derived Linux Sharing peer, not the
Rust application. Run stock Android to Linux and Linux to stock Android for
each supported attachment type and decision. The Linux peer must observe the
selected medium, byte count, SHA-256, terminal state, and cleanup through its
test protocol.

There is no documented Wi-Fi bridge that places a Linux host process on the
new shared emulator Wi-Fi link. Google's guide describes the shared link
between emulator instances. The older host redirection APIs forward selected
TCP or UDP ports and do not supply link-layer discovery. A port forward cannot
honestly prove mDNS, Wi-Fi Direct negotiation, hotspot association, or medium
selection. [documented interconnection scope](https://developer.android.com/studio/run/emulator-networking-interconnect)

Bluetooth has one experimental route. Netsim permits host-side Bumble devices
to join the emulator's virtual Bluetooth network, and Bumble can exercise BLE
and Classic protocols. Bumble warns that emulator Bluetooth integration is
still evolving and calls custom-controller attachment an advanced use case
that may not be officially supported. A future lab may bridge a Netsim
controller into Linux BlueZ, but it cannot enter the required gate until a
pinned BlueZ-to-AVD reference self-test passes in both roles.
[Netsim device types](https://android.googlesource.com/platform/tools/netsim/+/refs/heads/main/guide/src/README.md),
[Bumble Android integration](https://google.github.io/bumble/platforms/android.html)

If no supported bridge lets stock Quick Share discover the Linux reference
peer, preserve the traces and mark this cross-host stock route unsupported.
Do not inject an already connected stream, replace stock Quick Share with the
probe, or claim that AVD-to-AVD radio tests prove the missing path.

## What the environment can prove

With the control matrix green, the AVD environment proves:

- one pinned Google Play build can run stock Quick Share in both directions;
- the emulator's documented AVD-to-AVD BLE, Classic, NSD, and Wi-Fi Direct
  paths work independently of Quick Share;
- the public Nearby Connections service can interoperate in both endpoint
  roles on its selected route;
- the closed Quick Share UI accepts the tested file, text, URL, decision, and
  cancellation scenarios on that image;
- a Linux reference route works only if the separate cross-host self-test
  actually passes.

It cannot prove compatibility with every Android version, Google Play services
build, OEM fork, chipset, driver, firmware, radio environment, account/contact
trust mode, background policy, or stock medium-selection decision. It also
cannot force or observe every Quick Share upgrade through a public Google API.
Those limits are permanent parts of the claim, not missing assertions that UI
automation can fix.

## Recommended repository targets

Keep provisioning separate from timed tests:

- `make android-license` runs the interactive license review.
- `make android-provision` fetches and verifies exact archives and builds the
  probe without starting the test timer.
- `make android-seed` performs the networked first boot, records closed Google
  component versions, passes AVD-to-AVD stock controls, and saves snapshots.
- `make android-up` restores both snapshots and reports lifecycle time.
- `make test-android-radios` runs the AVD radio controls.
- `make test-android-connections` runs the public API control in both roles.
- `make test-android-quick-share` runs the stock AVD-to-AVD matrix.
- `make test-android-linux-peer` exists only after the cross-host reference
  route passes its own self-test.
- `make android-down` collects failure artifacts, stops both AVDs and Netsim,
  and reports lifecycle time.

The provisioner should fail before application work if Quick Share is absent.
That is the honest result: the exact Google Play image is available, but its
stock Quick Share capability is not established by any published package
label or Android Emulator guarantee.
