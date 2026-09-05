# Programmatic connection testing for a Rust Quick Share endpoint

Research date: 2026-09-02

## Verdict

Yes. Existing tools can give strong, repeatable evidence that a Rust endpoint sends and receives according to the protocol and drives Linux networking correctly. There is no single Quick Share emulator that proves compatibility with every Android phone.

The bidirectional additions, tool limits, licenses, and exact readiness matrix are pinned in [bidirectional programmatic verification](research/bidirectional-programmatic-verification.md).

The best test setup has four independent layers:

1. Google's C++ code is the protocol oracle.
2. Google and Rust test vectors cover framing and cryptography.
3. Linux virtual Bluetooth and Wi-Fi devices exercise the real BlueZ, NetworkManager, and socket adapters.
4. A small manual physical-device beta can catch firmware and Google Play Services behavior that software simulation cannot reproduce.

This distinction matters. A D-Bus mock can prove that the daemon handles `Adapter1.Powered = false`; it cannot prove that a Samsung phone sees the BLE advertisement. A virtual radio can prove that BlueZ advertises, discovers, and opens an RFCOMM or L2CAP channel in either local role; it cannot prove that Google's closed Android endpoint makes the same choice. Cross-process tests against Google's C++ implementation provide the strongest protocol evidence available without a phone.

## Confirmed test seams and project gate

The project owner confirmed these seams on 2026-09-02. Tests must observe behavior through them rather than call private application code.

### Local control seam

The Omarchy plugin controls the local endpoint through a versioned JSON protocol over a Unix socket. The interface covers:

- opening a timed visibility window
- accepting or rejecting an incoming transfer
- discovering peers for an outbound transfer
- offering one or more files to a selected peer
- cancelling an active inbound or outbound transfer
- reading endpoint, discovery, and transfer status

The interface includes its ordering rules, error responses, compatibility policy, and behavior when either process restarts. The QML plugin and automated tests use the same interface.

### Transfer seam

The local endpoint reports typed events for:

- visibility, listening, and outbound discovery state
- an incoming transfer request
- an outbound offer and the peer's decision
- the authentication PIN
- the selected connection medium
- directional transfer progress
- completion, rejection, cancellation, or failure
- transport and operating-system resource cleanup

Inbound tests verify received files by declared size and SHA-256. Outbound tests verify the bytes observed by the independent peer against the source file's size and SHA-256. Both verify the terminal event and cleanup result without inspecting private state.

### Connection seam

Automated connection tests approach the local endpoint through the same Bluetooth and network mechanisms used by a phone. Depending on the test level, the peer is Google's reference implementation, a deterministic simulator, a virtual radio peer, or an Android virtual device. A physical phone participates only in manual compatibility testing.

The required connection types are:

- BLE advertising, discovery, GATT, and the BLE data path
- Bluetooth Classic discovery and its RFCOMM or L2CAP data path
- same-LAN discovery and TCP
- Wi-Fi hotspot creation or join and TCP
- Wi-Fi Direct group-client join and TCP
- bandwidth upgrades between an initial low-bandwidth connection and a high-bandwidth connection
- fallback when an offered medium fails or disappears

Each connection type must cover the local endpoint as initiator and responder where the protocol permits both roles. Each role covers discovery, encrypted connection establishment, transfer, consent or rejection, cancellation, failure recovery, fallback, and cleanup. Upgrade-capable paths also cover successful and failed migration without corrupting or duplicating a payload.

Tests may use a test configuration to force a medium or failure. They may not bypass that medium by injecting an already-connected stream into private application code.

### Oracle seam

Google's pinned C++ implementation communicates through a separate, versioned test protocol over framed stdin and stdout or a Unix socket. The oracle reports decoded protocol states and typed events. It does not expose implementation logs as test results.

Tests compare semantic states, decoded messages, authentication results, payload bytes, and terminal outcomes. They compare raw bytes only when Google supplies a deterministic reference value. Random keys, endpoint identifiers, and ciphertext are verified through live interoperability or decoded meaning.

### Development and release thresholds

Application behavior may start when its tools and upstream inputs are pinned, independent fixtures cover its states and payloads, deterministic adapters pass the shared contracts, and the underlying operating-system transport self-tests are green. The first failing behavior test permits only the code needed to pass it. Add missing fixtures and contracts with the slice that first needs them.

Release evidence has the stronger threshold described in [bidirectional programmatic verification](research/bidirectional-programmatic-verification.md). Each claimed connection type must prove the real adapter, encrypted connection, exact payload, terminal outcome, and cleanup in every supported role. A known-red reference-peer experiment is a diagnostic, not a development blocker or compatibility result.

The pre-push gate includes every admitted reproducible check. Admit an experimental route after it passes repeatably within its time budget. Physical devices remain a separate manual procedure. Every failure preserves structured, privacy-safe evidence of the failed protocol stage.

## Test-support policy

Tests should have first-class support code. Repeated setup must move into named helpers, builders, factories, fixtures, fakes, or narrowly scoped stubs and mocks. The support code must make the behavior under test easier to see. It must not hide which connection path, failure, or user decision caused the result.

Use this preference order when replacing a dependency:

1. the real dependency in an isolated environment
2. a virtualized real system, such as `bluetoothd` over `btvirt` or NetworkManager over `mac80211_hwsim`
3. a fixture produced by an independent reference implementation
4. a behavioral fake
5. a fixed-response stub
6. a mock, only when the interaction itself is part of the confirmed interface

### Fast feedback and simulator fidelity

Routine TDD starts with in-process behavior tests and deterministic doubles at external seams: discovery, connection outcomes, time, randomness, payloads, decisions, storage, and upgrades. Rare failures should remain millisecond-fast.

Fast and simulator layers consume the same semantic scenarios where they overlap. Scenarios describe peer actions, observations, faults, payload facts, and outcomes, never project-module calls. Contract cases run against each double and its real, virtualized, or oracle-backed adapter before the double is trusted.

Simulator and oracle suites cover fewer combinations but cross real framing, process, operating-system, virtual-radio, and reference-peer boundaries in both roles. They keep the larger fast matrix honest. Mock only external boundaries; assert public events, results, payload bytes, cleanup, and terminal outcomes.

### Helpers

Helpers should express actions in the project vocabulary, such as `open_visibility_window`, `offer_file`, `accept_inbound_share`, `reject_outbound_offer`, `drop_upgrade_channel`, and `expect_cleanup`. They may coordinate setup and return observed results. They should not contain the assertion that makes a test pass unless they are clearly named assertion helpers.

Keep helpers small enough that a reader can understand a test without opening several files. A helper must not choose a transport, accept a transfer, advance time, or retry silently unless its name says so.

### Builders and factories

Use valid-by-default builders for advertisements, offline frames, paired-key messages, introductions, payloads, transfer decisions, and failure scenarios. A test should override only the fields relevant to the behavior it names.

Factories should create isolated resources such as temporary receive and source directories, Unix sockets, endpoint identifiers, virtual adapters, network namespaces, and oracle processes. They own cleanup and must fail the test if cleanup does not finish.

Builders and factories must not duplicate protocol calculations from the application. Expected encoded bytes come from pinned Google fixtures or independently worked values. Otherwise the test could repeat the same defect as the application.

### Fixtures

Commit stable fixtures when they are small and legally redistributable. Each generated fixture records:

- the exact upstream repository and commit
- the generator and command used
- the decoded meaning of the data
- which values were normalized because they depend on time or randomness
- the expected result and protocol stage

Keep byte fixtures separate from semantic scenario fixtures. Byte fixtures prove exact framing. Scenario fixtures prove ordered behavior. Regenerating fixtures must produce a reviewable diff, and the local fixture-validation gate must reject unrecorded changes.

Payload fixtures should cover empty, small, chunk-boundary, multi-chunk, large, malformed, truncated, duplicate, and path-hostile inputs. Tests verify their size and SHA-256 rather than trusting filenames.

### Fakes

Prefer fakes for stateful external systems. Planned fakes include deterministic clocks and randomness, a scripted Google peer, BlueZ and NetworkManager D-Bus services, disk-space reporting, and controlled transfer storage.

A fake must model only the behavior used by the local endpoint, including failures and state transitions in both directions. It should reject impossible operations instead of accepting every call. Where practical, run the same contract cases against the fake and its real or virtualized counterpart. This keeps a pleasant fake from drifting away from BlueZ, NetworkManager, or the Google oracle.

Do not fake a connection medium in a test that claims to verify that medium. BLE, Bluetooth Classic, LAN, hotspot, and Wi-Fi Direct gates must eventually cross the real operating-system interface or virtual radio described by the connection seam.

### Stubs

Use stubs for one fixed response or event, such as insufficient disk space, an adapter capability, a D-Bus error, or a user rejection. Keep them local to the test when possible. If a stub needs mutable state, branching, or a sequence of responses, it has become a fake and should be named and implemented as one.

### Mocks

Use mocks sparingly. They are appropriate for observable interactions at confirmed external seams, such as verifying that an advertisement is unregistered, a temporary network profile is removed, or a terminal event is emitted once.

Do not mock calls between our own modules. Do not assert private call counts or internal ordering. Those tests would couple the suite to an implementation that does not exist yet and could remain green while public transfer behavior is broken.

Mock expectations should describe a protocol or resource-lifecycle rule, not the current arrangement of functions. Prefer a fake plus a final state assertion when either approach can prove the same behavior.

### Shared quality rules

- Name support types honestly with `Builder`, `Factory`, `Fixture`, `Fake`, `Stub`, or `Mock`.
- Give every fake clock and random source an explicit seed or starting value.
- Never use an unbounded sleep to synchronize a test.
- Preserve the seed, scenario, oracle transcript, packet capture, and minimized input for any failure.
- Keep support code under test directories or test-only build targets so none of it enters the published binary.
- Delete support code that no longer makes a test clearer or no longer protects a confirmed behavior.

## File-size limits

Project-authored text files have hard size ceilings:

- application, library, tool, script, build, configuration, and documentation files may contain no more than 500 physical lines
- test files and test-only support files may contain no more than 800 physical lines
- every `AGENTS.md` file may contain no more than 500 physical lines

The local file-size gate must count physical lines, including blank lines and comments. A file at the limit passes. A file over the limit fails. Renaming a source file or moving it into a test directory does not change which limit applies.

The limits are ceilings, not targets. Split a growing file at a real responsibility or confirmed seam before it reaches the limit. Do not create numbered `part1` and `part2` files or move unrelated functions into a catch-all utility file merely to satisfy the count.

Generated sources, dependency lockfiles, vendored upstream code, and binary fixtures are exempt because the project does not maintain their lines by hand. Their generator or upstream revision must remain pinned and reproducible. Project-authored fixture descriptions, generators, adapters, and test scenarios are not exempt.

## Rust formatting and lint policy

The workspace must pin an exact Rust toolchain with the `rustfmt` and `clippy` components. Application code, tests, examples, benchmarks, build scripts, and project-authored tools follow the same policy. Local quality gates treat every diagnostic enabled by the project as an error.

Project-specific structural linting follows the [strict ast-grep policy](ast-grep-strict-rust-policy.md). The project writes every ast-grep rule explicitly, treats every match as an error, and accepts no generated or remote default rule set.

The required checks are equivalent to:

```bash
cargo fmt --all -- --check
RUSTFLAGS="-Dwarnings" cargo check --workspace --all-targets --all-features
RUSTFLAGS="-Dwarnings" cargo test --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features --no-deps -- \
  -D warnings \
  -D clippy::all \
  -D clippy::pedantic \
  -D clippy::nursery \
  -D clippy::cargo
RUSTDOCFLAGS="-Dwarnings" cargo doc --workspace --all-features --no-deps
```

`cargo fmt --check` must fail on any formatting difference. Rustfmt is a formatter rather than a lint engine, so its enforcement is all or nothing. See the official [`cargo fmt` documentation](https://doc.rust-lang.org/cargo/commands/cargo-fmt.html).

`-D warnings` promotes every emitted compiler, Clippy, and rustdoc warning to an error. It does not enable lints that the toolchain leaves at `allow` by default. The project must therefore keep a toolchain-specific lint manifest generated from `rustc -W help`, Clippy's lint list, and rustdoc's lint list. Every stable lint shipped by the pinned toolchain must appear in that manifest.

The default classification is `deny`. An exclusion is permitted only when two lints contradict each other, a lint cannot apply to this type of project, or the tool itself documents that the lint group must not be enabled wholesale. Each exclusion records the lint name, toolchain version, concrete reason, and chosen competing rule. A toolchain update cannot be accepted until the local lint-inventory gate regenerates the inventory and every new lint has been classified.

Clippy's `all`, `pedantic`, `nursery`, and `cargo` groups run as errors. Restriction lints are reviewed and enabled individually. Clippy explicitly says that [`clippy::restriction` must not be enabled as a whole](https://doc.rust-lang.org/stable/clippy/index.html#clippy) because its lints can contradict each other and other Clippy groups. The manifest should enable every restriction lint that remains coherent after those conflicts are resolved.

Project code may suppress a lint only at the narrowest item that needs it. Every suppression includes a `reason` that explains why the code is correct and why the preferred form cannot be used. Workspace-wide or module-wide `allow` attributes are prohibited unless the lint manifest records the same exception. Prefer `#[expect(..., reason = "...")]` when the diagnostic is expected at one known location, since the compiler then reports when the expectation becomes stale.

The project uses `deny` rather than `forbid` so a narrow, justified exception remains possible. Rust documents how `deny`, `forbid`, command-line flags, and local attributes interact in the [`rustc` lint-level reference](https://doc.rust-lang.org/stable/rustc/lints/levels.html).

These checks apply to the workspace, not third-party dependencies. Cargo caps dependency lints, and Clippy's `--no-deps` keeps upstream warnings from making our build depend on code we do not maintain. Generated and vendored sources follow the provenance and reproducibility rule above.

## Makefile policy

The repository must provide a root `Makefile` as its project-level task runner. Developers and local hooks use the same public targets.

TDD slices, semantic commits, targeted pre-commit checks, affected-test selection, and full pre-push verification follow the [development workflow](development-workflow.md).

Cargo remains the Rust build system. Make targets delegate Rust compilation, formatting, linting, documentation, and tests to Cargo. The Makefile must not call `rustc` directly or duplicate dependency, feature, profile, or target selection that belongs in Cargo configuration.

`make help` is the authoritative list of public targets. Add a stable aggregate
only when it has executable responsibilities and a targeted child command for
failure feedback. Do not document planned targets as if they already exist.

`verify` is the complete non-release quality gate. Smaller targets may compose
into it. It includes every admitted reproducible check supported by the pinned
C++ oracle, protocol fixtures, deterministic network simulation, D-Bus
services, fault injectors, virtual Bluetooth controllers, virtual Wi-Fi radios,
and Android routes. An experiment becomes admitted after its reference control
passes repeatably. Checks that need Linux capabilities must run through a
prepared local VM, container, or namespace without an interactive privilege
prompt. `build` produces the normal distributable artifact and runs only after
`verify` passes in the pre-push hook. No Make target or Git hook may require a
physical phone.

### Fail-fast and runtime budget

Every quality gate must fail fast. This includes formatting, compiler checks, linting, documentation, tests, fixture validation, file-size checks, oracle checks, fuzz smoke tests, packaging checks, and connection checks. A gate stops at its first unhandled failure and returns a non-zero status with enough diagnostics to reproduce the failure.

A top-level aggregate command may take more than 60 seconds because it composes several gates. This exception applies only to top-level Make entry points, such as `make verify`, and top-level Cargo commands that aggregate several targets or test binaries. It does not exempt any child gate hidden behind those commands.

Every child test gate in an aggregate has a hard budget of 60 seconds of wall-clock time in its documented local reference environment. Sibling gates are measured separately. If any one of them exceeds 60 seconds, split it into stable, directly runnable gates based on a real responsibility, test suite, connection type, or test environment. Keep the aggregate command as the convenient way to run all of them.

Prepared-environment startup and teardown are separate, measured lifecycle targets. Their time is not charged to the child test gate. Aim to start an already provisioned environment in 30 seconds and keep startup or teardown under 60 seconds where practical. One-time downloads, compilation, image creation, and Android SDK installation belong to explicit provisioning targets and are not test execution. A test timer begins only after its environment reports ready and ends before teardown starts. Complete verification starts one prepared oracle reference container before the selected medium and bandwidth-upgrade child gates, then stops it after the last child. Each child keeps its own timeout while reusing the warm Bazel server.

Lifecycle commands must emit their measured wall-clock time. The Toxiproxy environment, for example, is provisioned once with `make proxy-provision`, started and readiness-checked by `make proxy-up`, and stopped by `make proxy-down`. `make test-proxy-toxiproxy` starts it before the timed self-test and tears it down after the timer ends.

The same budget applies to scripts and tools invoked during the test phase by Make or Cargo. Wrapping slow assertions in another process or running them in the background does not reset or avoid the limit. Parallel execution may reduce aggregate time, but it does not make an over-budget child test gate acceptable.

Run cheap, deterministic gates before expensive gates when dependencies allow it. The local gate runner must enforce the 60-second child-gate timeout and report the timed-out gate by name. `make help` must list the targeted command for every child gate so a developer can rerun only the failed area.

### Fast-feedback entries in AGENTS.md

Any change that creates a gate, splits a gate to meet the runtime budget, or renames a gate must update the root `AGENTS.md` in the same change. Add or replace a concise entry under a `Fast feedback gates` heading. Each entry should take no more than two lines and must give the exact Make or Cargo command plus the feature area or failure class it checks.

Prefer the public Make target when one exists. Mention a required service, privilege, virtual device, or fixture only when the command cannot run without it. Remove stale commands instead of keeping a history. The section is a routing table that lets an agent choose the narrowest relevant gate before running a top-level aggregate.

Keep recipes short. Complex orchestration belongs in named, project-authored scripts with direct tests where appropriate. Make invokes those scripts and propagates their exit status. Recipes must be non-interactive in hooks, fail on the first unhandled error, and preserve the diagnostics required by the test policy.

The Makefile follows the 500-line project limit. Splitting work into scripts must follow real responsibilities rather than hiding an oversized Makefile. Make is a development and local-hook dependency only. Building a release artifact must not make `make` a runtime requirement for plugin users.

## Google's test facilities

Google's public Nearby tree already contains most of the reference side we need.

### MediumEnvironment and OfflineSimulationUser

[`MediumEnvironment`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/internal/platform/medium_environment.h) is an in-process simulated hardware environment. It connects multiple fake devices and implements advertising, discovery, data links, and notifications for Bluetooth Classic, BLE, Wi-Fi LAN, Wi-Fi Direct, and Wi-Fi hotspot. It can install a simulated clock, then fast-forward timers without waiting in real time.

[`OfflineSimulationUser`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_simulation_user.h) wraps a complete Connections endpoint. Google's [`offline_service_controller_test.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_service_controller_test.cc) uses two such endpoints to advertise, discover, connect, accept or reject, transfer byte and stream payloads, cancel, disconnect, and request bandwidth upgrades. The suite runs combinations of BLE, Bluetooth Classic, and Wi-Fi LAN.

This code is useful in two ways:

- Run the upstream suite unchanged at the pinned Google commit. It tells us whether our chosen reference still builds and records the expected behavior.
- Build a small C++ executable around the same classes. Give it a versioned stdin/stdout or Unix-socket test protocol so Rust tests can command the Google endpoint and observe semantic events.

`MediumEnvironment` cannot directly connect to a Rust process. Its fake radios live inside the C++ process. We need a thin bridge at the endpoint-channel boundary or a test-only TCP medium. That bridge is project code, but the connection logic on the far side remains Google's implementation.

Do not compare only log text. The bridge should emit typed records for discovered endpoint, connection state, authentication token, selected medium, frame type, payload progress, and disconnect reason. Inject deterministic randomness where possible. For values that remain random, compare decoded fields and state transitions rather than raw ciphertext.

Google's simulator is not a hardware model. It also has some timing sensitivity. One upstream test documents a rare advertisement race in the fake environment. Use it as the behavioral oracle, not as the sole release evidence.

The public Google source tree references an internal `gloop` thread-ID helper, task-pool API, WebRTC tree, and UTF helpers that it does not publish. The oracle workspace supplies minimal, tracked Linux compatibility headers for thread identity, callback execution, and the WebRTC types present in shared headers; it also selects Google's own public-build UTF stub and limits the Linux binary to the five supported medium suites. AWDL, generic Wi-Fi state, and WebRTC are outside this matrix. The overlays do not implement or emulate discovery, advertising, sockets, or data links, so Bluetooth and Wi-Fi medium behavior remains Google's code. The overlays and their use are validated as part of the pinned oracle build.

### Sharing-layer fixtures and live peers

Google's [`FakeNearbyConnectionsManager`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/fake_nearby_connections_manager.h) lets Sharing tests inject connections, raw authentication tokens, incoming payloads, completion events, and bandwidth-upgrade results. [`incoming_share_session_test.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/incoming_share_session_test.cc) covers file, text, URL, APK, invalid-size, insufficient-storage, accept, cancel, failure, timeout, and upgrade cases.

Google's [`outgoing_share_session_test.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/sharing/outgoing_share_session_test.cc) covers attachment-to-payload mapping, introductions, accept and reject responses, sequential payload sending, timeout, cancellation, and release of delayed file completion after prior payload success. These are semantic oracles built on fakes, not live peers.

The Rust endpoint cannot import these C++ fakes. We should mirror their scenarios in a language-neutral corpus. Each case can be a checked-in file containing:

```text
initial configuration
share direction and local protocol role
ordered inbound Sharing frames and payload events
clock advances
expected outbound frames
expected transfer state and filesystem result
```

A generator can run each case through the Google implementation and write the expected result. Rust consumes the same case. This is more maintainable than translating hundreds of Google test functions by hand, and regeneration against a new pinned commit makes upstream behavior changes visible in review.

Live Sharing interoperability uses a pinned, test-only control wrapper around the Apache-2.0 Linux Nearby fork's `nearby_sharing_cli`. The wrapper must choose peer, accept or reject, cancel at a named stage, report selected media, and emit byte counts and SHA-256. It enters the gate only after reference-to-reference self-tests pass. Add pinned NearShare as an implementation-diverse same-LAN peer; it cannot stand in for Bluetooth, hotspot, or Wi-Fi Direct coverage.

The test-only Google-derived reference lab uses Docker Compose only for its fixed two-peer topology, health, isolated bridge, case mounts, and teardown; a project runner owns its sealed multi-stage build, scenarios, assertions, evidence, and cleanup.
Each peer uses a private bus, real NetworkManager and Avahi, and an adapter-shaped BlueZ fake that carries no traffic. The lab proves Wi-Fi LAN only; Bluetooth still requires virtual radios.

### Reference message and state comparison

This matrix uses Google Nearby `588531995decf09500870ed4d2e1ac6740a3e338`.

| Transition                 | Pinned reference                                                                                                                                                                                                                              | Current local boundary                                                                                                                                                                       |
| -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Account-free pairing       | Both roles send encryption, read encryption, send result, then read result; public `UNABLE` may continue.                                                                                                                                     | Same ordering and policy. Certificate-backed trust modes remain unsupported.                                                                                                                 |
| Inbound attachment         | Introduction precedes consent; every declared payload must succeed; disconnect before completion fails.                                                                                                                                       | File, text, URL, and app-file receive paths validate ID, size, kind, and offset. Streams and Wi-Fi credentials are unsupported.                                                              |
| Outbound `FILE`            | Final `LAST_CHUNK` establishes Connections payload success. With safe-disconnect negotiated, matching `PAYLOAD_ACK` separately signals safe disconnect. Sharing delays final completion until peer closure, with a 60-second failure timeout. | Local safe-disconnect is disabled. `send_file` establishes payload-write success, then bounded control drain determines final completion, failure, or cancellation.                          |
| Outbound `BYTES`           | BYTES has no payload-received acknowledgement. Final `LAST_CHUNK` establishes payload success, followed by the same delayed Sharing completion lifecycle.                                                                                     | `send_bytes` establishes payload-write success, not terminal share completion. Progress reaching total remains nonterminal while bounded control drain runs.                                 |
| Post-confirmation dispatch | Registered response, payload, bandwidth-upgrade, keepalive, and disconnection processors run. Known unregistered V1 types 7 through 12 are logged and ignored.                                                                                | Types 7 through 12 continue without implementing their optional features. Setup types 1 and 2 and missing, zero, or unknown discriminators remain strict errors by intentional local policy. |
| Upgrade channel switch     | New-channel `CLIENT_INTRODUCTION` and acknowledgement are plaintext; old-channel `LAST_WRITE` and `SAFE_TO_CLOSE` are encrypted. Authenticated old-channel events may interleave.                                                             | The independent green checks cover exact plaintext and encrypted wire bytes, ordering, budgets, disconnect, upgrade failure, and keepalive interleaving.                                     |
| Terminal failure           | Protocol, transport, rejection, cancellation, and timeout remain distinct.                                                                                                                                                                    | A public daemon/CLI regression proves that `ProtocolError::reason()` drives the same stored terminal reason and recovery through daemon state, JSON control/plugin, and CLI.                 |

Final `LAST_CHUNK` establishes Connections payload success only; Sharing stays `DelayComplete` or `PendingComplete` until peer closure, or fails when its 60-second retention expires.
After local payload state retires, valid late ACK, cancel, and error controls are ignored; pre-LAST cancellation remains authoritative. A future negotiated ACK follows consumer write and flush.

The old-image Google-derived `FILE` retry decoded `BANDWIDTH_UPGRADE_RETRY` 12 before `unexpected_frame_type`; the exact fresh inbound 20-byte URL plus Retry 12 and keepalive is green in 26.64 seconds.
Fresh outbound-content is green in 48.7 seconds: TEXT completed in 21.3 seconds, then both peers completed the exact 20-byte URL with Retry 12 and keepalive in 22.3 seconds. Immediate closure may race the ACK write and complete through pinned [`EndpointManager`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/endpoint_manager.cc#L174-L327) removal and `OnDisconnected`; a held-open raw peer requires the exact keepalive acknowledgement. Independent Core tests cover both timings.
On commit `f489346`, the rebuilt current image passed all ten LAN children as 12 Node tests in 559.98 seconds including provisioning; each child took 25.85 to 49.28 seconds. The matrix covers both roles for FILE, TEXT, exact 20-byte URL plus Retry 12, rejection, cancellation, true socket-EOF failure, and Retry 12 plus FILE.
The bounded post-completion drain accepts EOF or reset close while preserving protocol, cryptographic, cancellation, and other I/O failures; its deterministic reset regression is green. The ten-check Quick Shell plugin release gate is also green. Advertisement loss remains nonterminal, and none of these checks proves stock Android behavior.

Rust loopbacks, raw peers, Google Sharing fixtures, the Google-derived Linux
peer, virtual radios, and the public Android Connections probe each cover only
their stated seam. None is a stock Quick Share emulator. Physical-phone tests
in both roles remain required for release compatibility claims.

### UKEY2 interoperability tests

UKEY2 has an especially good existing pattern. Google's [`ukey2_shell.cc`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/src/securegcm/ukey2_shell.cc) is a command-line compatibility endpoint with length-prefixed stdin and stdout messages. It can act as either role, report handshake messages and the verification string, encrypt, decrypt, and return the session-unique value. Google's [`Ukey2CppCompatibilityTest.java`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/javatest/com/google/security/cryptauth/lib/securegcm/Ukey2CppCompatibilityTest.java) launches that C++ process, completes both initiator and responder handshakes, compares the authentication string, then encrypts in one implementation and decrypts in the other.

Copy the shape of that test exactly, substituting Rust for Java. The Rust suite should run both roles against Google's C++ shell and verify:

- all three handshake messages
- the displayed authentication token
- C++ to Rust and Rust to C++ encryption
- sequence-number rejection, replay rejection, truncation, and bad MAC handling
- session-unique equality

This is stronger than golden ciphertext alone because ephemeral keys change on every run. Google's C++ D2D tests also cover saved session state and bidirectional encryption in [`d2d_connection_context_v1_test.cc`](https://github.com/google/ukey2/blob/10fc737aa901e873a3367e7e26b88eb01cd55d69/src/main/cpp/test/securegcm/d2d_connection_context_v1_test.cc).

### Parsers and fuzzing

Google ships an LLVM libFuzzer entry point that passes arbitrary bytes to the Connections frame parser in [`offline_frames_fuzzer.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/fuzzers/offline_frames_fuzzer.cc). Seed the C++ and Rust fuzzers with serialized frames from Google's [`offline_frames_test.cc`](https://github.com/google/nearby/blob/588531995decf09500870ed4d2e1ac6740a3e338/connections/implementation/offline_frames_test.cc), then share every minimized input found by either implementation.

Use [`cargo-fuzz`](https://github.com/rust-fuzz/cargo-fuzz) for the Rust targets. At minimum, fuzz:

- advertisement v1 and v2 decoding
- length-delimited offline frames
- UKEY2 handshake parsing
- encrypted D2D messages
- payload headers and chunks
- paired-key and introduction frames

"Does not crash" is an insufficient invariant. Parsers must cap allocations, reject trailing or truncated data consistently, never write outside the transfer staging directory, and reach the same semantic result as Google's parser for the shared corpus.

Use Tokio's test clock for retry, keepalive, and timeout tests. Its [`time::pause` and `time::advance`](https://docs.rs/tokio/latest/tokio/time/fn.pause.html) APIs let tests advance virtual time instead of sleeping. The production state machines need clock and randomness inputs so tests do not depend on wall-clock scheduling or random endpoint IDs.

### Tokio Turmoil

[`Turmoil`](https://github.com/tokio-rs/turmoil) is a deterministic simulator from the Tokio project. It runs several Rust hosts in one thread with simulated TCP, UDP, DNS, multicast, broadcast, and time. A seed makes message timing and failure choices repeatable. Tests can partition links, hold and inspect in-flight messages, add latency, fail messages, crash a host, then repair the network.

Turmoil is a particularly good fit for the medium-independent Connections state machine and LAN path. Put the socket operations behind a small internal interface so production selects Tokio sockets and tests select `turmoil::net`. Then run scenarios such as:

- mDNS advertisement arrives before or after the endpoint is ready
- UKEY2 frames split across arbitrary TCP reads
- connection loss after introduction but before acceptance
- payload interruption followed by reconnect or clean failure
- low-bandwidth channel remains alive while a TCP upgrade stalls
- two upgrade candidates race, one disappears, and the old channel survives
- peer holds frames past keepalive or handshake deadlines

This gives fast, reproducible exploration of timing cases that are awkward with subprocess proxies. It remains a Rust-only network model. It cannot run the C++ oracle inside its simulated network, drive BlueZ or NetworkManager, or validate kernel socket behavior. Use it to find state-machine bugs, then keep the C++ and Linux integration gates separate.

## Linux Bluetooth testing

Two first-party projects cover different levels of the Bluetooth path.

### BlueZ btvirt and hciemu

BlueZ includes the `btvirt` Bluetooth controller emulator and `hciemu` test library. Its [`test-runner`](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/doc/test-runner.rst) can boot a small QEMU guest, start D-Bus, `bluetoothd`, `btmon`, and `btvirt`, then run a test. BlueZ itself uses `hciemu` to test real kernel RFCOMM and L2CAP sockets in [`rfcomm-tester.c`](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/tools/rfcomm-tester.c) and [`l2cap-tester.c`](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/tools/l2cap-tester.c).

The prepared radio environment boots a pinned Debian kernel, D-Bus, real `bluetoothd`, and two pinned `btvirt` controllers inside a KVM guest. Keeping VHCI inside the guest prevents the host `bluetoothd` from claiming test controllers or disrupting a user's live Bluetooth devices. `make bluetooth-radio-up` and `make bluetooth-radio-down` measure lifecycle separately from test time.

`make test-bluetooth-controller` checks both isolated controllers through BlueZ. `make test-bluetooth-ble` exchanges exact bytes in both directions between BlueZ D-Bus GATT and a pinned Bumble peer. `make test-bluetooth-classic` does the same through Linux RFCOMM and Bumble. When the Rust adapter exists, extend these gates to assert:

- BLE advertisement registration and cleanup
- scan and adapter-loss signals
- GATT service registration and reads or writes
- Bluetooth Classic profile registration
- RFCOMM and L2CAP connection setup, data, close, and reconnect
- daemon recovery when the controller disappears

This exercises the kernel and BlueZ APIs that D-Bus mocks skip. It still emulates the controller, so it will not reveal vendor firmware quirks or antenna coexistence problems.

### Bumble and Netsim

Google's [Bumble](https://github.com/google/bumble) is a Python Bluetooth host and controller stack. It implements BLE and Bluetooth Classic protocols including L2CAP, ATT, GATT, SDP, and RFCOMM. A Bumble virtual controller can attach to BlueZ through Linux VHCI, where BlueZ sees it as a normal virtual adapter. The project documents that setup in its [Linux guide](https://google.github.io/bumble/platforms/linux.html#using-bumble-with-bluez).

Bumble is convenient for a programmable peer. It can advertise exact byte strings, expose the GATT characteristics expected by Nearby, open Classic sockets, deliberately disconnect mid-frame, or report odd controller capabilities. It should test our BlueZ adapter and error handling. Do not use a hand-written Bumble peer as the protocol oracle, because any mistake duplicated in the Rust and Python code would pass.

Bumble can also join Android Emulator's Netsim virtual Bluetooth network. Netsim lets several Android Virtual Devices and Bumble clients communicate over one virtual radio. Google's [Android integration guide](https://google.github.io/bumble/platforms/android.html) warns that emulator Bluetooth support is recent and evolving, and calls custom-controller attachment an advanced path that may not be officially supported. First give this route a pinned, repeatable self-test. Once that self-test passes reliably, its connection cases are required in the pre-push suite. Until then, report the route as unsupported rather than claiming Android-backed Bluetooth coverage.

Google's [RootCanal](https://android.googlesource.com/platform/packages/modules/Bluetooth/+/refs/heads/master/tools/rootcanal/README.md) is another virtual Bluetooth controller and is the controller behind Android virtual-device Bluetooth. It can expose HCI over TCP and place several controllers on simulated LE and Classic physical links. BlueZ's [`btproxy`](https://github.com/bluez/bluez/blob/3a2d543c4c21d9c1dab246d46d76a11996a69bf2/tools/btproxy.c) can connect a remote HCI controller to Linux VHCI. Once this route passes a pinned self-test, its distinct Android Emulator to Linux BlueZ cases enter `make verify`. RootCanal explicitly omits real physical-layer scheduling, so it does not replace manual hardware testing.

## D-Bus behavior without radios

[`python-dbusmock`](https://github.com/martinpitt/python-dbusmock) starts a private bus and provides standard [`bluez5.py`](https://github.com/martinpitt/python-dbusmock/blob/45885bf940b3f90f72570b202e8cc3b56d3b0506/dbusmock/templates/bluez5.py) and [`networkmanager.py`](https://github.com/martinpitt/python-dbusmock/blob/45885bf940b3f90f72570b202e8cc3b56d3b0506/dbusmock/templates/networkmanager.py) service templates. Tests can add adapters, devices, Wi-Fi access points, active connections, properties, signals, method errors, and disconnects. Calls are recorded and can be asserted from any language.

This is a good fast test for every push. It can prove that Rust makes the right D-Bus calls, handles signals in the wrong order, retries on service restart, unregisters advertisements, and removes temporary network profiles.

The prepared private-bus image is pinned by base digest and Debian snapshot. `make test-dbus-bluez` runs upstream advertisement, pairing, monitor, and agent cases through `bluetoothctl`; `make test-dbus-networkmanager` runs upstream Wi-Fi, connection, state, and settings cases through `nmcli`. Environment startup and teardown are measured outside both test timers.

The bundled templates are incomplete for this project. The BlueZ template lacks the full GATT application model, and the NetworkManager template does not model its Wi-Fi P2P interfaces. Extend them with only the methods and signals the daemon uses. NetworkManager also ships its own [`test-networkmanager-service.py`](https://github.com/NetworkManager/NetworkManager/blob/main/tools/test-networkmanager-service.py), which is useful as a more detailed reference service but likewise is not a radio simulator.

Never count a D-Bus mock test as Bluetooth or Wi-Fi interoperability. It validates our use of a service API.

## Linux Wi-Fi and LAN testing

For the actual Linux Wi-Fi path, use [`mac80211_hwsim`](https://wireless.docs.kernel.org/en/latest/en/users/drivers/mac80211_hwsim.html). It creates any number of virtual 802.11 radios. Frames transmitted on one radio are delivered to other radios on the same channel, and normal user-space programs use the interfaces without special code. The hostap project has a large automated [`tests/hwsim`](https://w1.fi/cgit/hostap/tree/tests/hwsim/) suite, including Wi-Fi Direct discovery, group formation, services, invitations, persistence, and concurrency.

A prepared local VM can run two hwsim radios with real NetworkManager or wpa_supplicant instances. The pre-push hook must start it without an interactive privilege prompt and test these cases:

- same-LAN DNS-SD discovery followed by TCP connection
- hotspot group owner and client association
- Wi-Fi Direct discovery and group-client join
- wrong passphrase, group-formation timeout, and peer disappearance
- bandwidth upgrade from a synthetic low-bandwidth channel to TCP
- cleanup after cancellation, crash, and suspend simulation

For ordinary TCP fault tests, [Toxiproxy](https://github.com/Shopify/toxiproxy) can inject latency, bandwidth limits, resets, timeouts, sliced writes, and early close through an HTTP-controlled proxy. It is useful for framing and migration tests because it can split one protocol frame across many reads or cut a payload at an exact byte count. It does not emulate mDNS, Wi-Fi association, or radio loss. Use hwsim and network namespaces for those.

Use `tc netem` in separate network namespaces for seeded UDP, multicast mDNS, and IP delay, loss, corruption, duplication, reordering, and rate limits. Use `wmediumd` beneath IP with `mac80211_hwsim` for association, asymmetric link, and 802.11 loss or delay. Every injector self-test proves a clean control, observes the requested fault in a capture or endpoint result, restores the route, then proves another clean control.

The pinned network environment is provisioned with `make network-provision`; this compiles wmediumd twice and rejects nondeterministic output. `make network-up` and `make network-down` report warm lifecycle times independently of test execution. The targeted gates cover control-fault-control at both the IP and 802.11 layers, real same-LAN and hotspot association in both endpoint roles, and Wi-Fi Direct discovery with Omarchy in the promised group-client role. Every association gate proves TCP payload delivery in both directions. Run them with `make test-network-netem`, `make test-network-wmediumd`, `make test-network-lan`, `make test-network-hotspot-client`, `make test-network-hotspot-owner`, and `make test-network-wifi-direct-client`.

## Android virtual devices

The Android Emulator is more useful than it used to be. Google's current [network capability table](https://developer.android.com/studio/run/emulator-networking) lists Bluetooth Classic and BLE for API 31 and later. Version 36.5 and later puts emulator instances on a shared virtual Wi-Fi network with Network Service Discovery and Wi-Fi Direct support, according to the official [multi-device networking documentation](https://developer.android.com/studio/run/emulator-networking-interconnect). Android's Netsim also supplies virtual Bluetooth controllers for AVD and Cuttlefish instances.

That does not automatically produce a reliable Quick Share test peer. The open `google/nearby` repository is the desktop C++ implementation, while Android Quick Share is delivered through Google software and has no public headless test API. A Google APIs system image supplies Play services for the public probe without the Play Store. A Play Store image may expose the stock product roles, but UI automation, account state, server-side flags, and image updates make that a weak automated compatibility signal.

Use Mobly only to orchestrate pinned AVDs, artifacts, a test-only Android probe, and [UI Automator](https://developer.android.com/training/testing/other-components/ui-automator). The probe's public Nearby Connections API can advertise, discover, connect, accept, reject, send a file, and cancel in both roles, but it does not prove the Sharing product or force a medium. Stock Quick Share cases drive the visible UI to send to Omarchy and accept or reject a file from Omarchy.

The stock route enters `make verify` only after an AVD-to-AVD control and an AVD-to-Linux transfer pass repeatable self-tests in both directions. Pin the emulator, image digest, Play services, locale, display, Mobly, UI test stack, and probe APK. If the Linux bridge or UI remains unreliable, record the unsupported route and keep it out of compatibility claims.

The September 2026 admission attempt is recorded in [the Android test-lab research](research/android-test-lab-existing-solutions-2026-09.md). Both pinned API 36 image variants reached the snippet control, but public Nearby advertising remained pending despite enabled radios, location, and granted permissions. The Google APIs variant met the warm lifecycle target; the route remains outside `make verify` until its AVD-to-AVD control passes repeatably.

Cuttlefish can share virtual Bluetooth and Wi-Fi media between instances and provides controls for Rootcanal and wmediumd in its [multi-device connectivity guide](https://source.android.com/docs/devices/cuttlefish/connectivity). Add any distinct Cuttlefish connection case to `make verify` once its pinned self-test is repeatable. Its size or startup cost is not a reason to omit a supported case. Split the work into child gates that meet the 60-second rule.

Cloud Android test farms do not solve the peer problem. They can run code on a hosted phone, but they do not place that phone beside our Linux Bluetooth controller or Wi-Fi Direct interface. They remain useful for testing a companion Android app, which this project does not plan to ship.

## Recommended local verification stack

A simulator route becomes required when its pinned environment passes a reference self-test and can reproduce a promised connection path or failure. Add it to `make verify`; do not leave it as an optional check. If a route cannot pass its own self-test, record it as unsupported and do not claim that coverage.

### Required before every push

1. Run Rust unit tests with deterministic clock and randomness.
2. Import Google's parser fixtures and UKEY2 compatibility cases. Run short `cargo-fuzz` smoke jobs on all external byte boundaries.
3. Run live Rust-to-C++ UKEY2 in both roles.
4. Run language-neutral inbound and outbound Connections and Sharing traces generated by the pinned C++ oracle.
5. Run Google's MediumEnvironment and OfflineSimulationUser scenarios for each supported medium and upgrade path with endpoint roles swapped.
6. Run seeded Turmoil scenarios for LAN discovery, connection, transfer in both directions, upgrade, partition, recovery, and timeout behavior.
7. Run D-Bus adapter tests on a private bus with extended BlueZ and NetworkManager templates.
8. Run live Sharing in both directions against the pinned Google-derived wrapper and the diverse LAN peer.
9. Run Toxiproxy TCP, netem UDP and IP, and wmediumd 802.11 fault cases after their control-fault-control self-tests.
10. Run the Rust daemon in both roles with real `bluetoothd` over BlueZ btvirt or hciemu and with a Bumble VHCI peer.
11. Run real NetworkManager or wpa_supplicant over `mac80211_hwsim` for both transfer directions on LAN, hotspot, and Wi-Fi Direct.
12. Run every RootCanal, Netsim, Cuttlefish, Android probe, and stock Quick Share AVD route whose direction-specific self-test has proved repeatable.

Each child gate must meet the 60-second limit. Gates that need Linux capabilities or virtual radios run inside the prepared local environment without prompting or changing the host during pre-push.

### Required periodically and before a release candidate

1. Build the pinned Google Nearby source and regenerate inbound and outbound oracle traces. Fail if behavior changes without an approved corpus update.
2. Fuzz for longer periods and preserve every minimized failure as a regression input.

### Manual physical-device compatibility

No automated gate, Git hook, or Make target runs this check. When physical phones are available, manually test both file-transfer directions plus inbound text and URL, verification-code comparison, cancellation, Bluetooth fallback, LAN upgrade, hotspot, and Wi-Fi Direct. Community testers can supply this evidence because the project has no internal device lab. Record the stage, selected medium, timeout or error code, Android version, and radio chipset while omitting filenames, peer names, addresses, keys, and payload contents.

## What this can prove

The automated stack can prove all of the following with high confidence:

- Rust parses and emits the same protocol messages as the pinned Google source.
- UKEY2 and D2D encryption interoperate live with Google's C++ code.
- inbound and outbound connection, payload, consent, cancellation, timeout, and upgrade state transitions match the reference scenarios.
- the daemon uses current BlueZ and NetworkManager APIs correctly.
- the Linux kernel paths can advertise, discover, connect, transfer in both directions, fail, and clean up over virtual radios.
- malformed or adversarial inputs do not crash the daemon, allocate without bounds, escape receive staging, or make unsafe outbound source reads.

It cannot prove that every Android release, Google Play Services build, phone vendor, Bluetooth controller, Wi-Fi driver, or power-management policy behaves the same way. A modest real-device beta remains part of the compatibility strategy. The simulations make that beta focused and diagnosable instead of being the first time the protocol is tested.
