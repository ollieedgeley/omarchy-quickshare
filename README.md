# Omarchy Quickshare infrastructure

## Private desktop environment

This environment prepares a real Omarchy desktop from the checksum-pinned 4.0.1
ISO in `tests/environments/desktop/environment.json`. It does not install the
Quickshare application or include application source in the reusable base identity.

### Prerequisites

Use a Linux x86-64 host with readable and writable `/dev/kvm`, Python 3.11 or
newer, GNU Make, QEMU with KVM and `virtio-vga` support, OVMF firmware, curl,
OpenSSH, OpenSSL, dosfstools, mtools, ImageMagick 7 (`magick`) and Tesseract. Preparation needs
network access for the ISO and guest packages, and disk space for the ISO, a
40 GiB sparse virtual disk, and writable overlays. There is no software CPU
emulation fallback.

On Arch Linux, the QEMU installation must include both the `virtio-vga` and
`virtio-gpu` device modules. On Omarchy, install the matching packages with:

```sh
omarchy pkg add qemu-hw-display-virtio-vga qemu-hw-display-virtio-gpu
```

If using private modules, set `QEMU_MODULE_DIR` to a directory containing both
matching QEMU device modules. Modules from a different QEMU build
may not load. Set `MCOPY` to a private `mcopy` executable if mtools is not on
`PATH`. These overrides are inherited by preparation and launch commands.

OVMF is discovered in common distribution paths. Otherwise set both
`DESKTOP_OVMF_CODE` and `DESKTOP_OVMF_VARS` to matching firmware files.

### Prepare and retain named labs

Run from the repository root:

```sh
make desktop-prepare
make desktop-start DESKTOP_LAB=dev
make desktop-status DESKTOP_LAB=dev
make desktop-logs DESKTOP_LAB=dev
make desktop-inspect DESKTOP_LAB=dev
make desktop-stop DESKTOP_LAB=dev
```

Preparation installs the desktop, waits for the installed system and desktop
session, records runtime package versions, and publishes a base only after clean
shutdown. A compatible base is reused. Its fingerprint includes the manifest,
the recorded desktop preparation inputs, firmware contents and QEMU version. Lab names and
resource allocations do not change that identity.
A preparation source change requires preparing the new identity before booting.
Guest package repositories remain live, so a fresh installation is not a promise
of identical package versions. The prepared base records what was installed.

`DESKTOP_LAB` defaults to `dev` and must match `[a-z][a-z0-9-]{0,31}`.
The first start creates its writable overlay and private firmware variables.
Repeated healthy start reuses the same process, UUID, SSH port and writable disk.
After stop, start resumes that disk with the same UUID and SSH port. It never
creates a replacement for incompatible or incomplete retained state. Prepare the
current inputs and choose a new name instead. Earlier unnamed `active.json` and
`guests/<uuid>` artifacts remain historical and are not adopted.

Readiness requires the user D-Bus session, an active Hyprland monitor, an answering
Omarchy shell and a running QEMU machine. A running PID alone is not ready.
Status reports live `health` as `ready`, `unhealthy`, `stopped` or `absent`.
It includes retained inputs and resources; inspect also attempts a screenshot.
QMP, SSH and screenshot failures are reported independently, so a broken component
does not hide the other diagnostics. Logs print the last 64 KiB of each host log
and bounded guest journals if SSH responds. These diagnostic commands succeed
when they report unhealthy state; read `health`, not their exit status.

Use the exact SSH configuration path printed by start or status:

```sh
ssh -F /absolute/guest/directory/ssh.config desktop
```

Replace the example path with the reported path. VNC is available through the
reported local Unix socket, not a public TCP listener.

Stop explicitly when finished. Failures retain the guest and diagnostic files;
commands do not silently clean up a failed session. Stop targets only the owned
QEMU process and preserves bases and overlays. It tries guest shutdown, then ACPI,
and finally QMP quit if necessary. Preparation never publishes a forcibly stopped
base.

Preparation uses a separate cache lock and `preparation.json`; lab start and stop
lock only the selected name. Read-only diagnostics do not wait for lifecycle
locks. A failed installation remains accessible with `make desktop-prepare-inspect`
and can be stopped with `make desktop-prepare-stop`. These commands never select
a named lab. Downloads and verified bases are shared between names; writable
disks, firmware, SSH forwards, QMP/VNC sockets and lifecycle locks are not.

### Native deployment

Build both native executables and deploy them into a running lab:

```sh
make native-build
make native-deploy DESKTOP_LAB=alpha
```

Builds use the locked release profile and one Cargo job by default. Each build
retains source and toolchain identity, artifact checksums and its build log under
`.cache/test-env/desktop/native/`. `NATIVE_COMPONENT=all` is the default;
`cli` and `daemon` select one component. Each selection has a latest successful
build pointer. `NATIVE_ARTIFACT=/absolute/build/directory` selects exact retained
bytes instead.

```sh
make native-build NATIVE_COMPONENT=cli
make native-deploy DESKTOP_LAB=alpha NATIVE_COMPONENT=cli
make native-build NATIVE_COMPONENT=daemon
make native-deploy DESKTOP_LAB=alpha NATIVE_COMPONENT=daemon
```

`DESKTOP_NATIVE_PROFILE=dev` selects an identified development build; the default
is `release`. `DESKTOP_NATIVE_TARGET=<triple>` selects a target supported by the
installed toolchain; the default is the effective compiler's host target.
Metadata records the actual Cargo executable and freshness, effective profile,
toolchain/configuration hashes and cache compatibility identity. Artifact files
are private read-only copies, separate from reusable compiler output.

Deployment verifies selected bytes and atomically replaces each selected
executable over pinned SSH. A daemon deployment also installs its unit and
starts or restarts `omarchy-quickshare.service`. Readiness uses the versioned
socket directly and checks the loaded daemon executable against the artifact.
The daemon-only path does not invoke the CLI.

CLI-only deployment preserves the daemon's state, PID and loaded bytes. Both
paths preserve the compositor and shell processes, and neither starts or
recreates a guest. Every attempt retains its outcome, component hashes,
copy/install/readiness logs and available service journal in
`labs/<name>/deployments/<uuid>/`. Failed deployment leaves the lab accessible.

### Panel deployment

```sh
make panel-deploy DESKTOP_LAB=alpha
make panel-deploy DESKTOP_LAB=alpha PANEL_SOURCE=/absolute/plugin/source
```

Panel deployment requires an unlocked guest and a running native service. It
uses the release export inventory, snapshots the QML, validates it with the
installed Omarchy validator, and replaces only this plugin. It restarts the
Omarchy shell, not the native service or guest. A registry rescan alone left an
old component executing during verification, so it is not used as reload proof.

The live widget must answer a fresh nonce with its stamped revision and component
URL. Installed file hashes must match the snapshot, and the native service PID
must remain unchanged. Attempts retain snapshots, identities and journals under
`labs/<name>/panel-deployments/<uuid>/`. The previous plugin remains in the guest
staging directory recorded by the attempt. Invalid QML fails deployment but
leaves SSH and the native service available. Redeploy valid source
to restore the panel. For unattended UI work, run `omarchy toggle idle stay-awake`
inside the disposable guest. This does not change the host desktop.

### Independent reference peer

```sh
make desktop-peer-prepare
make desktop-peer-start DESKTOP_LAB=alpha
make desktop-peer-status DESKTOP_LAB=alpha
make desktop-peer-inspect DESKTOP_LAB=alpha
make desktop-peer-logs DESKTOP_LAB=alpha
make desktop-peer-stop DESKTOP_LAB=alpha
```

Preparation uses the existing pinned Nearby Linux source/build path. Missing
source inputs must be prepared before it can build. Start rejects an image whose
input fingerprint differs and never rebuilds it implicitly. Application and panel
edits do not change the reference image inputs.

Each lab gets an internal Docker network, reference receiver and Ethernet relay.
The QEMU process connects to the relay's private bridge address; no relay port is
published. A second guest NIC joins only this network. The peer disables checksum
offload on its own veth so the raw relay carries complete wire packets. This does
not change host or guest firewall policy.

Readiness requires actual Sharing DNS-SD records and a reachable advertised TCP
listener from the guest. It is not transfer-success evidence. Repeated start
retains healthy containers. Inspect reports container state and guest network
details; failed starts retain their error and resources. Stop detaches the
selected guest NIC and stops its containers. `make desktop-peer-remove
DESKTOP_LAB=alpha` also removes that lab's peer containers and network, but retains
its received files. Other labs and prepared images are not removed.

### Retained transfer scenario

```sh
make desktop-scenario DESKTOP_LAB=alpha
```

The scenario requires a running desktop, deployed native application and ready
reference peer. It discovers the named peer through the public CLI and sends a
private, uniquely named synthetic binary file. Success requires the application
to report the matching completed share, independent receiver offer/accept/complete
events, and the exact received filename, size and SHA-256.

Each attempt retains its payload, command evidence, peer events, component
identities and `result.json` under `labs/<name>/scenarios/<uuid>/`. Neither success
nor failure shuts down the guest or reference service. Before an independent run,
reset resolves only a proved scenario-owned share through public cancel/dismiss
operations. Unrelated active work blocks execution. Reset restarts the owned
receiver inside its existing container and verifies new receiver generation,
idle state and actual DNS-SD/TCP readiness. It does not recreate the container,
network, relay or VM, or restart the native service.

The pinned receiver cannot reliably reuse its completed-session state; its next
listener bind failed during verification. Restarting just that receiver is the
explicit reset operation, not a hidden retry during transfer. The runner selects
the current DNS-SD peer identity rather than assuming its display name is unique.
Prior received files and evidence remain intact. Reset and transfer assertions
share one lab lock, and failed setup prevents a new send.

To exercise a controlled failure and then inspect/retry:

```sh
DESKTOP_SCENARIO_FAULT=peer-unreachable make desktop-scenario DESKTOP_LAB=alpha
make desktop-inspect DESKTOP_LAB=alpha
make desktop-scenario DESKTOP_LAB=alpha
```

The first command deliberately returns failure. After reset and discovery, it
adds an unreachable host route for only the recorded private peer inside the
guest. The real connection attempt fails; the runner records the native outcome
and removes only its exact owned route in `finally`. It does not change host
networking, the guest's default route, or security policy. Unknown fault modes
are rejected.

`fault.json` and `fault-commands.jsonl` retain the action, route state, observed
native outcome and restoration result without the disposable sudo credential.
The failed public share remains inspectable until the next independent reset.
A normal retry gets a new attempt directory and cannot overwrite that failure.
Cancellation, native failure and successful delivery remain distinct outcomes.

### Build cache ownership

Storage lifecycles are separate:

| Data              | Location and ownership                                                                                           |
| ----------------- | ---------------------------------------------------------------------------------------------------------------- |
| Downloads         | Desktop ISO under `desktop/downloads`, pinned sources under `sources`, Cargo downloads under its configured home |
| Compiler output   | `desktop/native/cargo-target` and the BuildKit Bazel cache                                                       |
| Runtime artifacts | Read-only desktop bases, identified native build directories, content-addressed reference images                 |
| Mutable lab data  | Named guest disks, `peer-cases` and per-attempt `scenarios` evidence                                             |

Paths above are relative to `.cache/test-env/` unless stated otherwise. Native
builds hold `desktop/native/build.lock`; Cargo still decides which compilation
outputs are compatible. Reference provisioning holds `nearby-linux/provision.lock`
through downloads, context construction, build and publication, including direct
Node invocations. It builds one canonical Compose service because both runtime
peers share an image tag. Concurrent publishers previously produced different
service labels under that same tag.

The reference image's final stage contains runtime libraries and services, not
LLVM, GCC, Cargo, Bazel or CMake. The Omarchy desktop keeps its representative
stock runtime. Cache reuse never replaces deployed-artifact identity checks.

### Lifecycle checks

To check repeated start and stopped-disk reuse:

```sh
make desktop-start DESKTOP_LAB=dev
make desktop-status DESKTOP_LAB=dev
make desktop-start DESKTOP_LAB=dev
make desktop-status DESKTOP_LAB=dev
make desktop-stop DESKTOP_LAB=dev
make desktop-start DESKTOP_LAB=dev
make desktop-status DESKTOP_LAB=dev
```

The two running starts must report the same PID, UUID, disk and SSH port.
After stop/start the PID may change; the UUID, disk and SSH port remain.
To check two independent owners, with enough available host memory:

```sh
make desktop-start DESKTOP_LAB=alpha
make desktop-start DESKTOP_LAB=beta
make desktop-status DESKTOP_LAB=alpha
make desktop-status DESKTOP_LAB=beta
make desktop-stop DESKTOP_LAB=alpha
make desktop-status DESKTOP_LAB=beta
make desktop-stop DESKTOP_LAB=beta
```

Their processes, disks, writable firmware, SSH ports and sockets must differ.
Stopping `alpha` must leave `beta` ready.

To simulate an unhealthy runtime without killing or replacing it, pause `alpha`
through its owned QMP connection. Start both names first. These commands affect
only the selected UUID:

```sh
make desktop-start DESKTOP_LAB=alpha
make desktop-start DESKTOP_LAB=beta
DESKTOP_LAB=alpha python3 -c '
import sys
sys.path.insert(0, "tests/environments/desktop")
import lab_lifecycle, vm_control
vm_control.qmp(lab_lifecycle.active(lab_lifecycle.directory()), "stop")
'
make desktop-status DESKTOP_LAB=alpha
make desktop-logs DESKTOP_LAB=alpha
make desktop-inspect DESKTOP_LAB=alpha
make desktop-start DESKTOP_LAB=alpha DESKTOP_BOOT_TIMEOUT=5
make desktop-status DESKTOP_LAB=beta
DESKTOP_LAB=alpha python3 -c '
import sys
sys.path.insert(0, "tests/environments/desktop")
import lab_lifecycle, vm_control
vm_control.qmp(lab_lifecycle.active(lab_lifecycle.directory()), "cont")
'
make desktop-start DESKTOP_LAB=alpha
make desktop-status DESKTOP_LAB=alpha
make desktop-stop DESKTOP_LAB=alpha
make desktop-stop DESKTOP_LAB=beta
```

While paused, `alpha` must remain running but unhealthy with QEMU status `paused`.
Its repeated start must fail without changing its process or disk. Its retained
logs and QMP inspection remain available despite bounded SSH timeouts. `beta`
must stay ready. After `cont`, `alpha` must become ready with the same ownership.

### Host capacity

Defaults: one guest CPU, 2048 MiB guest RAM and 1024 MiB interactive host headroom.
`make desktop-resources` reports the joint VM, peer and worker budget. Admission
reserves at least one host CPU and uses `MemAvailable`, never swap capacity.
Unresident VM/container allocations remain charged; already resident memory is
not counted twice.

| Variable                       |      Default | Minimum |
| ------------------------------ | -----------: | ------: |
| `DESKTOP_CPUS`                 |            1 |       1 |
| `DESKTOP_MEMORY_MIB`           |         2048 |    1024 |
| `DESKTOP_HOST_HEADROOM_MIB`    |         1024 |     512 |
| `DESKTOP_HOST_HEADROOM_CPUS`   |            1 |       1 |
| `DESKTOP_REFERENCE_MEMORY_MIB` |         3072 |    1536 |
| `DESKTOP_INSTALL_TIMEOUT`      | 3600 seconds |       1 |
| `DESKTOP_BOOT_TIMEOUT`         |  600 seconds |       1 |

For an explicitly smaller allocation, apply these settings to preparation and
start. A running lab rejects a changed CPU/RAM request without replacing it.
Stop first to resize on resume:

```sh
export DESKTOP_CPUS=1 DESKTOP_MEMORY_MIB=1536 DESKTOP_HOST_HEADROOM_MIB=768
make desktop-prepare
make desktop-start
make desktop-inspect
make desktop-stop
```

The retained Omarchy 4.0.1 guest completed a verified transfer with all QEMU
threads restricted to one host CPU and 1536 MiB allocated RAM. Reports distinguish
advertised guest vCPUs from the union of actual thread affinity masks. Missing or
unstable affinity evidence falls back to the full guest allocation.

Native compilation reserves 1536 MiB and pins Cargo and its descendants to one
allowed host CPU. Native memory is an admission budget, not a hard memory limit;
build evidence records peak RSS. Scenario orchestration reserves one CPU and
128 MiB in addition to its VM and peer; visible OCR work reserves 512 MiB.
The short admission lock does not serialize independent work. Inherited process
tokens keep orphaned
children charged; PID start times and boot identity prevent PID-reuse mistakes.

Reference compilation uses a checkout-owned BuildKit container with a one-CPU
hard limit, the selected memory limit and no additional swap. Bazel uses one job,
a 512 MiB JVM heap and a scheduling budget 1024 MiB below the container limit.
The builder stops after the operation but keeps its cache. A surviving builder
remains charged and must be inspected/stopped explicitly. Build records retain
actual cgroup memory observations; native records retain child peak RSS and CPU
time. Reservations and measurements are different quantities.

### Retained storage

```sh
make desktop-cleanup-inspect DESKTOP_LAB=alpha
make desktop-cleanup DESKTOP_LAB=alpha
```

Dry-run is also the default for `retention.py`. Completed synthetic payloads
expire after 168 hours; unreferenced native payloads and identified bases after
720 hours. Active work, current-share source files, referenced bases, latest or
deployed native artifacts, unknown ownership and original result/build evidence
remain protected. Legacy bases without output-integrity manifests are retained.
New bases publish their integrity manifest before the final readiness marker.

Explicit compiler-cache pruning is separate and disabled by default:

```sh
python3 tests/environments/desktop/retention.py apply \
  --prune-native-cache --native-cache-profile dev --native-cache-age-hours 720
```

It uses the native build lock and recorded host/target profile directories,
through Cargo's supported cleanup command. Other profiles remain intact.
Artifact readers use a separate shared lock, so deploying immutable bytes does
not wait for compilation. Verified downloads and extracted tools are kept.

Every cleanup retains its plan, ownership decisions, actual removals and timing
under `desktop/cleanups/<uuid>/`. Logical bytes and allocated file blocks are
reported separately. Guest-file blocks are not claimed as reclaimed host qcow2
space; shared copy-on-write extents are not exclusive physical disk usage.

### Operation identity and recovery

The standalone CLI is `omarchy-quickshare`; the user service runs
`omarchy-quickshare-daemon`. Submission acknowledgement means accepted work,
not completed delivery. Keep the request ID printed by the CLI when acceptance
is uncertain, and query it rather than sending the content again:

```sh
omarchy-quickshare operation <request-id> --json
omarchy-quickshare status --json
omarchy-quickshare protocol-version
```

An operation query reads retained metadata; it neither retries nor resumes a
transfer. An unknown outcome does not prove the daemon never accepted the
request. The daemon keeps its bounded operation journal at
`${XDG_STATE_HOME:-$HOME/.local/state}/omarchy-quickshare/operations.json`.
Do not delete it to resolve uncertain delivery.

After a restart, previously active work is reported as failed with terminal
reason `interrupted`. The remote outcome can be uncertain; check the recipient
before deliberately submitting new work. Neither client automatically resumes
or replays the payload. Saved preferences and committed received content are
separate from the operation journal.

Protocol 7 versions only the local daemon/CLI/panel JSON API, not Google's
Connections or Sharing wire protocol. Deploy compatible local components.
Snapshot responses contain sibling `snapshot`, `preferences`, and `operations`
fields. Submission envelopes require `request_id`; the CLI and panel generate
it. Deterministic `simulate` fixtures do not prove independent-peer compatibility.

### Fresh candidate installation

Build native bytes first, then assemble a local candidate without publishing it:

```sh
make desktop-candidate NATIVE_ARTIFACT="$NATIVE_ARTIFACT"
make desktop-clean-install \
  DESKTOP_CANDIDATE="$DESKTOP_CANDIDATE" \
  DESKTOP_LAB=clean-example \
  DESKTOP_BASE="$DESKTOP_BASE"
```

Set `NATIVE_ARTIFACT` to an identified native build directory,
`DESKTOP_CANDIDATE` to the directory printed by candidate assembly, and
`DESKTOP_BASE` to the selected prepared-base fingerprint. Candidate assembly
defaults to the latest successful native build when no artifact is supplied.
Installation requires a new lab name; it never adopts a retained owner.

To use an exact, already-exported plugin repository:

```sh
make desktop-candidate NATIVE_ARTIFACT="$NATIVE_ARTIFACT" \
  DESKTOP_PLUGIN=/absolute/clean-export-repo
```

The equivalent candidate helper option is `candidate.py --artifact <native-dir>
--plugin <clean-export-repo>`. Assembly copies and verifies that export without
re-exporting it, preserving private source-proof provenance. Assembly alone
does not prove a source-built candidate installs or runs in a fresh guest.

The candidate contains the real Arch package and a private Git export of the
plugin. The guest installs them with `pacman -U` and
`omarchy plugin add ... --enable --yes`. It uses the packaged user service,
not a development link or a replacement desktop. Verification checks absence
before installation, package and loaded executable hashes, the effective unit,
public native availability, plugin commit/files and the loaded QML component.

`desktop/clean-installs/<uuid>/installation.json` records the selected candidate,
base and fresh guest identity. Stage commands, logs and failed readiness probes
remain beside it. The standalone installer retains its guest on either outcome
for inspection; it does not silently retry installation in an existing owner.

### Visible journeys and disposable jobs

After standalone installation, start its independent peer and exercise the
installed panel:

```sh
make desktop-peer-prepare
make desktop-peer-start DESKTOP_LAB=clean-example
make desktop-ui-scenario DESKTOP_LAB=clean-example \
  DESKTOP_INSTALLATION="$DESKTOP_INSTALLATION"
```

Set `DESKTOP_INSTALLATION` to the absolute `installation.json` path reported by
the installer. The driver verifies the installed service and plugin, opens a
private synthetic-file directory, and sends pointer/keyboard input only through
that guest's owned QMP connection. It verifies the copied URI, the uniquely
selected peer, completed public share and exact received bytes. Sent and Done
must be observed on the panel; backend completion alone cannot pass the journey.
Original frames, OCR word bounds/confidence and input observations remain with
the result under `labs/<name>/scenarios/<uuid>/`.

Before file input, the driver closes only the installed plugin's identified
popup. It checks the private file-manager window's current focus and geometry
before input, uses Ctrl+A to select the payload directory's contents, and
requires visible selection feedback before Ctrl+C. The copied URI must still
identify exactly the one synthetic payload; extra selections fail the journey.

The default rejects a guest process that differs from the original installation
record. After explicitly restarting the same owned guest, use
`--reattest-installation` to retain fresh evidence without editing that record.
Use `--cli-unavailable` to prove the panel works while the installed CLI cannot
execute. These options are independent and can be combined:

```sh
python3 tests/environments/desktop/ui_scenario.py \
  --installation "$DESKTOP_INSTALLATION" \
  --reattest-installation --cli-unavailable
```

Re-attestation checks the original owner directory identity, guest UUID, storage
paths and backing image, plus the image files held by the live QEMU process.
It re-probes installed native bytes, units and the loaded plugin, recording
`installation-attestation.json` linked to the original manifest's SHA-256.
It does not invent a historical writable-image inode or checksum that the
original installation did not record.

The CLI proof records the selected executable's identity and mode, removes only
its execute bits after preparation, and checks it at framebuffer and input
boundaries through capture, send and dismissal. Public state comes directly
from the daemon socket. A `finally` block restores the exact mode before the
final native probe; `cli-unavailable.json` retains the checks and restoration.

For an isolated install, journey and cleanup in one command:

```sh
make desktop-clean-job \
  DESKTOP_CANDIDATE="$DESKTOP_CANDIDATE" DESKTOP_BASE="$DESKTOP_BASE"
```

The command prints its durable `desktop/jobs/<uuid>/job.json` before starting.
It creates a new owner, prepares the peer, exercises the installed panel, saves
evidence and removes only its disposable VM, writable disk, firmware, containers
and network. Passing and failing results remain separate from cleanup attempts.
Retained development labs, reusable inputs and other owners are not removed.

After a worker interruption, recover using that exact record:

```sh
make desktop-clean-recover DESKTOP_JOB="$DESKTOP_JOB"
```

Set `DESKTOP_JOB` to the absolute `job.json` path. Recovery refuses a live worker
or unproven ownership. It uses recorded boot/process identities and the same
owned cleanup path, retaining previous evidence. A successful recovery can still
return nonzero because the original failed or interrupted result is preserved;
inspect `cleanupAttempts` as well as `originalResult`.

### Shared desktop verification

`.github/workflows/desktop.yml` is manual-dispatch only. Shared execution has not
yet been verified. Dispatch, publishing and disclosure of its artifacts require
approval.

All dispatch inputs are required:

- `worker`: an approved fresh Ubuntu 24.04 x64 worker with KVM.
- `workspace_budget_gib`: reviewed physical peak storage for the ISO, base,
  overlay, Cargo and evidence, not the disks' virtual capacity.
- `docker_budget_gib`: reviewed physical peak storage for the pinned reference
  build and image. The budgets are added when both use the same filesystem.

The workflow does not provision a worker or supply a default worker label.
It serially reuses `desktop-prepare`, `native-build`, `desktop-candidate`,
`desktop-peer-prepare` and `desktop-clean-job`, including the installed-panel
journey rather than a CLI-send substitute.

A checkout-wide lifetime lock and durable coordinator/process identities protect
run and recovery ownership. Recovery refuses a live run and preserves the original
outcome separately from cleanup attempts. An always step attempts owned recovery;
complete worker loss still depends on ephemeral-worker teardown.

A bounded allowlist retains source, environment and candidate identities, result
JSON, logs, original PNGs and OCR TSV for 14 days. SSH keys, VM disks, developer
caches and publishing credentials are not included.

### Diagnostics and privacy

State lives under `.cache/test-env/desktop/`. `labs/<name>/state.json` identifies
each retained owner; `preparation.json` identifies the separate installer.
`preparations/<uuid>/` and `labs/<name>/` retain `serial.log`, `qemu.log`,
`qemu.pid`, `ssh.config`, `known_hosts`, QMP and VNC socket paths, and
`desktop.ppm` screenshots. Installer OCR also produces `desktop.png`.
`bases/<fingerprint>/` contains the immutable base disk, firmware variables,
private SSH key, pinned host key, `runtime.txt` and `prepared.json`.

The environment uses a restrictive file-creation mask and a loopback-only SSH
forward. First-contact host-key acceptance is restricted to owned preparation
guests. Disposable candidate keys are pinned only after the same connection
proves it reached the installed btrfs root, not the ISO. Runtime and overlay SSH
require that pin; host-key checking is not disabled globally.

The guest uses the disposable `omarchy` account and password, with a dedicated SSH
key. Do not expose it to untrusted users or use real credentials inside it. Disk
images, seed credentials, private keys, logs and screenshots are private local
artifacts. Do not commit or share the cache as routine diagnostics. Review and
redact any diagnostic material before sharing it.

## Focused tests

Install the locked JavaScript dependencies with `npm ci` using Node 24 or
Node 26 and newer. Vitest runs the retained tooling tests and orchestrates
cross-process scenarios; Rust and QML keep their native test runners.

```sh
make test-tooling
make test-release-tools
make test-environment-tools
node_modules/.bin/vitest run tools/gates/tests/source-manifest-contract.test.mjs
make test-rust-focused RUST_TEST_FILTER=contradictory_scenarios
make test-qml-components
```

The default Vitest configuration selects only fast tooling, release and
environment-helper tests. Release tests that simulate build or service commands
do not prove a real installation. The headless QML tests exercise the production
peer-choice component with controlled shell presentation dependencies, not a
running desktop or daemon.

Process scenarios use a separate configuration and require their real runtime
prerequisites. Select an already-ready retained lab explicitly:

```sh
make desktop-scenario DESKTOP_LAB=alpha
```

The scenario owns its attempt and temporary fault, not the guest or reference
service. Both remain available after passing and failing attempts. Disposable
`desktop-clean-job` verification invokes Vitest for the installed-panel journey;
the enclosing job still owns installation, evidence and cleanup. It also requires
the locked JavaScript dependencies on the host. Neither Vitest route replaces
explicit lab preparation, deployment or peer startup.

## Language formatting and size checks

Use clang-format 22.1.8 and Qt's qmlformat 6.11.2. `CLANG_FORMAT` and
`QMLFORMAT` override their executable paths. Install the additional pinned
dependencies into `.cache/tools`, without changing system packages:

```sh
make language-style-provision
make lint-language-style STYLE_FILES="path/to/source.py path/to/source.cc"
make format-native
make format-native-check
```

Omit `STYLE_FILES` to check all discovered files. Native formatting covers C++
and QML; it also runs through `format-tooling` and `format-tooling-check`.
Size checks run through the existing focused analyzer selection and
`lint-analysis-general`.

The policies are 80 columns, 500 source-file lines, 800 test-file lines and
50 function lines, using each dependency's counting semantics:

- ESLint owns JavaScript line, file and function limits.
- Ruff owns Python line width; rustfmt owns Rust formatting and Clippy owns
  Rust function length.
- Checkstyle owns non-JavaScript file lengths, Java method length and line
  width for Rust, C++, Java, QML and Bash. Test classification uses the
  existing analyzer path rules. Generated directories are excluded, matching
  the existing duplication-check policy.
- Lizard owns Python and C++ function length. Its standard complexity and
  parameter checks remain enabled.
- clang-format and qmlformat use an 80-column formatting target.
  clang-tidy also uses a 50-line function threshold in its existing scope.

These tools differ in their treatment of comments, blank lines and formatting
exceptions. There is no project-written counting or normalization pass.
Bash and QML function length is not enforced: the selected tools do not
provide that rule. Existing violations are reported, not baselined or hidden.
