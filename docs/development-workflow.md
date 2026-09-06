# Development, commit, and push workflow

Policy date: 2026-09-02

## Project rule

Build every behavior change with test-driven development through a confirmed seam. Commit each working vertical slice as soon as its targeted gates pass. Every commit follows Conventional Commits. Before a commit, hooks format and lint the staged change, then test the wider code area affected by it. Before a push, hooks run the complete local quality suite against the exact commit being pushed, then build that commit.

Local hooks are the project's automated verification. The pre-commit hook gives targeted feedback. The pre-push hook is authoritative. It verifies and then builds the exact commit being pushed. Hosted CI and automated release builds are outside the current scope. A future decision may add release automation that calls the same local targets, but it must not replace local verification or the source-build fallback.

Application and development-tool feedback have separate aggregates. `make verify-app` runs only Rust formatting, compiler checks, Rust diagnostics, ast-grep, and Rust tests. `make verify-tooling` runs the separate tooling and documentation formatting checks, every current ESLint core rule, static environment definitions, and fast tooling contracts; it does not start simulators or virtual devices. `make verify` adds strict cross-language analysis and every programmatic environment check. This keeps environment implementation out of the normal application loop without weakening pre-push verification.

## TDD cycle

One slice is one externally observable behavior through the local-control, transfer, connection, or oracle seam. Work in this order:

1. Name one behavior and its confirmed seam.
2. Add the smallest behavior test that should fail.
3. Run its narrow test gate and confirm that it fails for the intended reason.
4. Add only enough application code to make that test pass.
5. Run the same gate and confirm the new test passes.
6. Review the green change. Refactor only after the behavior works, then rerun the gate.
7. Run every targeted quality gate selected for the staged change.
8. Commit the test, implementation, and directly required fixture or support change together.

Do not write a horizontal batch of imagined tests followed by a batch of implementation. Do not mock project modules or inspect private application state. Expected results come from specifications, pinned Google fixtures, worked values, or observable outcomes rather than calculations copied from the implementation.

A behavior commit must build and pass its selected gates on its own. Do not push red tests, `WIP` commits, disabled tests, or speculative code. Pure documentation, build, and test-infrastructure changes do not need an invented application behavior test, but they must use the narrow gates for their own contracts.

## Semantic commits

For this project, "semantic commit" means [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/):

```text
<type>(<scope>)!: <description>
```

Use these types:

- `feat` for a new user-visible or public capability
- `fix` for a defect correction
- `test` for test or test-infrastructure behavior without application behavior changes
- `refactor` for a behavior-preserving code change
- `perf` for a measured performance improvement
- `docs` for documentation only
- `build` for toolchain, dependency, packaging, Make, or hook changes
- `chore` only when no more specific type fits

The scope names the affected domain or adapter, such as `transfer`, `protocol`, `bluetooth`, `lan`, `storage`, `oracle`, or `hooks`. Use `!` or a `BREAKING CHANGE:` footer when compatibility breaks. The subject states one completed change in the imperative mood, without a trailing period.

Examples:

```text
feat(transfer): accept an empty file payload
fix(storage): reject parent path components
test(oracle): cover a failed bandwidth upgrade
refactor(protocol): isolate frame decoding
build(hooks): select tests affected by staged files
```

A `commit-msg` hook validates the format. Each feature or fix commit contains its red-to-green test. Separate unrelated behavior into separate commits.

## Hook implementation policy

Use tracked Husky hook entry points and project-owned scripts. Husky is a development-only dependency that makes the repository hooks portable and discoverable; it is excluded from runtime and source-build artifacts. Hook files remain small and delegate to root Make targets:

```text
pre-commit  -> make pre-commit
commit-msg  -> make commit-msg COMMIT_MSG_FILE=<path>
pre-push    -> make pre-push
```

Git does not activate repository hook files merely because they are tracked. A documented `make hooks-install` target installs the pinned development dependencies, lets Husky configure this clone's hook path, verifies the tracked hook files, and prepares the staged-tree mirror. Hook installation never changes a user's global Git configuration.

Hooks are non-interactive, check-only, and fail on the first unhandled error. They must not format files, update snapshots, stage changes, create commits, contact GitHub, or push. A nonzero pre-commit result aborts the commit, while a nonzero pre-push result aborts the push, as defined by the [official Git hook contract](https://git-scm.com/docs/githooks).

Pre-commit retains the caller's exact Git index context internally, but removes
repository-local Git environment variables from Cargo and Node test children.
Pre-push removes the same variables from commands targeting its explicit
worktree. The shared helper uses `git rev-parse --local-env-vars` to identify
them, preserving transport and authentication variables. This prevents inherited
repository settings from redirecting child Git operations into the caller's
repository.

Normal development must not use `--no-verify`. It bypasses the project's only automated verification before GitHub receives the change.

## OMP per-edit policy

`lsp.formatOnWrite` remains disabled. Restart the OMP session after changing
the hook so OMP loads the updated project extension. The post-edit hook safely
fixes and then checks only the existing, regular, in-repository files changed
by that `write` or `edit` result. It runs one bounded exact-file pass: rustfmt
for Rust; ESLint then Prettier for JavaScript and TypeScript; Ruff lint then
Ruff format for Python; markdownlint then Prettier for Markdown; and Prettier
for JSON, JSONC, and YAML. Rust formatting stays file-scoped and does not run
Cargo or Clippy. Git hooks remain check-only.

Green results let the agent continue. Remaining errors return a tool error
for the agent to fix.

If a fix changes bytes, the result tells the agent to re-read those files
because earlier snapshot anchors are stale. Final exact-file checks collect
all remaining applicable errors. Missing tools, execution failures, timeouts,
and failure-log write errors fail closed. Only final unresolved outcomes
append private JSONL records to the ignored
`.cache/omp/post-edit-failures.jsonl`. Each record contains `timestamp`,
`language`, `tool`, `rule`, `count`, and `kind`; it contains no paths, source,
messages, commands, or tool output. The counts record each observation,
including repeated observations, rather than claiming a count of unique
defects.

## Staged snapshot

Staged quality paths are the added, copied, modified, renamed, or type-changed entries whose after-snapshot exists in the index. Staged source files are the non-test members of that set. Staged tests are directly staged test files. Extended tests are additional test endpoints selected by impact analysis.

`make hooks-install` creates the mirror from `HEAD`, or Git's empty tree before the first commit, and performs its initial full CodeGraph index. At the start of each pre-commit run, the hook refreshes the mirror's tracked files from the exact staged snapshot while preserving its local `.codegraph/` database, then runs `codegraph sync --quiet` inside the mirror. Routine pre-commit runs never perform a full re-index. A missing, corrupt, or incompatible mirror index fails with the exact setup command needed to rebuild it.
The hook checks that the mirror tree matches the Git index before trusting analysis. CodeGraph and compiler-backed staged gates run only inside that mirror. The dirty working tree and the repository root's developer-facing CodeGraph index are not pre-commit inputs.

Deleted files remain impact-analysis inputs even though exact-file formatting and linting cannot read them. Renames contribute both old and new paths.

## Pre-commit gate order

The top-level `make pre-commit` and affected-test `make pre-commit-test`
commands are aggregates and may exceed one minute. Each real child test gate
remains directly runnable, appears in `make help`, and retains one 60-second
timeout around its entire invocation. Neither aggregate adds a timeout.
Prepared-environment lifecycle time is measured separately under the
connection-test policy.

The public pre-commit sequence is prepare, structure, exact-file source
format/lint/ast/analysis, exact-file test format/lint/ast/analysis,
language-domain analysis, then affected tests.

Run child gates in this fail-fast order:

1. Prepare (staged snapshot validation, mirror refresh, CodeGraph sync, index
   match proof).
2. Structure (file-size limits for changed project-authored files).
3. Source gates: `pre-commit-source-format` (format-source),
   `pre-commit-source-lint` (lint-source), `pre-commit-source-ast`
   (ast-source), `pre-commit-source-analysis` (analysis-source).
4. Staged-test gates: `pre-commit-test-format` (format-tests),
   `pre-commit-test-lint` (lint-tests), `pre-commit-test-ast` (ast-tests),
   `pre-commit-test-analysis` (analysis-tests).
5. `pre-commit-domain-analysis` reruns applicable analyzers over every file in
   each repository domain touched by the staged change.
6. Affected tests via the `pre-commit-test` aggregate, serially and fail-fast:
   `pre-commit-test-libraries`, `pre-commit-test-app`, then
   `pre-commit-test-tooling`. Each computes the same full selection using
   `computeSelectionRecord` (records `{path,language,domain}` in
   stagedSources/stagedTests/extendedTests), `parseAffectedJson` (only affected
   test paths), and `packageSelection` (Rust owner/downstream packages).

The library/shared-contract child runs every selected Rust package outside
`crates/app`, including integration-only suites. The application child runs
the selected `crates/app` package, including its library target. Both retain
all-target, all-feature, locked Cargo tests and doc tests for packages with
libraries. The tooling child runs the selected runnable Node tests.
This split preserves the complete selected test union and the existing
60-second limit for each real child; it drops no selected tests.

Behavior development begins with the smallest in-process test at an external
seam. Deterministic fakes, stubs, and mocks provide routine feedback, while the
same semantic scenario is retained for the relevant adapter contract and
simulator or oracle suite. Broaden to those slower layers after the behavior is
green; use their results to correct a drifting test double rather than weakening
the shared scenario.

The hook prints each selected command before running it. It records staged sources, staged tests, extended tests, CodeGraph inputs and candidates, traversal count, fallback reason, selected Cargo packages, languages, domains, and runnable Node test paths in `.cache/gates/pre-commit-selection.json`.

### Formatting and linting granularity

Rustfmt can check staged `.rs` files directly in the staged-tree mirror. Cargo's formatter can select packages but does not expose a changed-file selector; see the official [`cargo fmt` options](https://doc.rust-lang.org/cargo/commands/cargo-fmt.html).

Prettier receives only changed files in the formats it supports. ESLint
receives only changed, existing JavaScript or TypeScript paths from the staged
mirror. Ruff receives only changed Python paths. These checks run separately
for staged sources and staged tests; they never widen to unrelated files.

The subsequent analysis phase dispatches only tools applicable to those exact
paths: jscpd for every file, cargo-machete for Rust owners, Knip for JavaScript
or TypeScript, Ruff and Vulture for Python, and clang-tidy plus Cppcheck for
C++. The domain phase repeats the same dispatch over complete touched
repository domains. Knip and cargo-machete necessarily evaluate their project
and package graphs while remaining inside the staged-tree mirror. Tests are
selected separately through CodeGraph and domain ownership.

Cargo check, Clippy, and rustdoc operate on packages rather than independent
source files. `packageSelection` maps changed Rust paths to their owning
workspace packages and transitive downstream workspace packages. Shared
workspace inputs or Rust paths without an owner conservatively select every
workspace package. `rust-lints.mjs` receives all selected `--package` arguments
in one invocation. Rust-analyzer's diagnostics command has no stable package
selector, so package-scoped pre-commit runs omit it; the workspace-wide
`make lint-rust` gate retains rust-analyzer diagnostics.

Use the complete lint flags from the Rust lint policy for every selected
package. Do not replace Clippy with LSP diagnostics. Clippy remains the
compiler-backed lint result, as described in the
[official Clippy usage guide](https://doc.rust-lang.org/clippy/usage.html).

### Strict cross-language analysis

`make analyzers-provision` installs pinned cargo-machete 0.9.2 and Vulture
2.16, and requires the exact system clang-tidy 22.1.8 and Cppcheck 2.21.1.
jscpd 5.1.2 and Knip 6.34.0 are exact npm development dependencies. Ruff
0.16.5 remains the single pinned Python lint installation.

`make lint-analysis` makes every finding fatal. jscpd recognizes all supported
languages and rejects duplication above 5%; a reported clone must span at
least 5 lines and 50 tokens so short syntax is not treated as production
duplication. Generated code, immutable fixtures, dependency trees, caches,
and lock data are excluded. Knip enables every configured issue category at
error severity and includes entry-point exports. Vulture reports only
100%-confidence dead code. Ruff retains all stable and preview rules.
The full analysis aggregate delegates to separately timed general, clang-tidy,
and Cppcheck child gates. This keeps each directly runnable gate within the
60-second contract without dropping analyzers from `make verify`.

Cppcheck enables all checkers, exhaustive analysis, inconclusive findings, and
a nonzero error exit across every project C++ translation unit and standalone
header. Missing external includes are the only global Cppcheck suppressions.
The standalone-header pass and exact-file checks also suppress
cross-translation-unit unused-function and unused-member noise. clang-tidy
enables the analyzer, bugprone, CERT, concurrency, C++ Core Guidelines, HICPP,
miscellaneous, modernization, performance, portability, and readability groups
with every emitted warning promoted to an error. It runs on the local
connections peer, whose pinned source tree supplies a reproducible compilation
context; Cppcheck remains the strict checker for the fixture generator, whose
full BoringSSL compilation graph exists only in the sealed Bazel image.

clang-tidy's production limits are directional maxima: cognitive complexity
15, branch complexity 10, 80 lines per function, nesting depth 4, 6
parameters, and 40 statements. Disabled checks are limited to incompatible
style pairs, synthetic-compilation metadata, external ABI and callback shapes,
or rules superseded by an enabled equivalent. The configuration records each
exception centrally; source-level suppressions are forbidden.

## Affected-test selection

Staged files define the change, not the test boundary. A project-owned selector collects staged paths and Git statuses, queries analysis tools, maps the result to Cargo, and invokes the tests. CodeGraph supplies graph data to that selector. It is not the selector or test runner.

Start with every staged test file. For staged production files, take the union of these inputs:

1. Run `codegraph affected --json` inside the synced staged-tree mirror, passing every staged path in a language CodeGraph indexes. It walks transitive dependents from each changed path and returns candidate test-file paths. The hook sets the traversal depth to 32 rather than relying on CodeGraph's default depth of five.
2. Repository-domain ownership always contributes its executable test suites. This supplies conservative coverage for unindexed files, empty CodeGraph results, query failures, deletions, and renames, and it may cross languages; for example, a QML plugin change selects its JavaScript release contracts.
3. Cargo metadata maps changed Rust files and candidate Rust tests to owning and downstream workspace packages.

The installed CodeGraph 1.6.0 implementation walks dependent files through resolved cross-file symbol edges. It does not walk dependencies, inspect Git, refresh its database, understand Cargo targets, or run tests. Its default path matcher also misses a relative path beginning with `tests/`. Pre-commit uses no `--filter` because `**/*.rs` makes the changed source a terminal match that prevents traversal. See [CodeGraph affected-test semantics](research/codegraph-affected-semantics.md) for the pinned behavior and sources.

Use a union, never an intersection. One input finding a test is enough to run it. CodeGraph widens the candidate test set through stored symbol edges. Repository domains cover non-indexed and cross-language seams. Cargo owns Rust package scope. None of them proves runtime behavior.

CodeGraph cannot name unit tests embedded in production `.rs` files or Rust doc tests because neither has a separate test-file path. Every selected Rust package therefore runs all-target and documentation tests even when CodeGraph returns integration-test candidates.

Fall back conservatively:

- If the staged-tree mirror or its index is missing, corrupt, or incompatible, stop with the setup command that recreates it.
- A CodeGraph query failure, invalid response, empty candidate set, unsupported path, deletion, or rename still retains repository-domain test selection.
- A Rust path without a package owner, or a workspace manifest, lockfile, toolchain file, or shared Rust input, selects every workspace package.
- A production Rust change always runs its owning and downstream packages even when CodeGraph returns no test endpoint.
- A hook, impact-selector, or gate change selects the tooling contract tests.

Cargo runs every selected package with `--all-targets --all-features --locked`, followed by documentation tests for packages that expose a library. Node runs each selected test file separately. Both loops stop at the first failure.

Contract fixtures cover staged-mirror reuse, exact index bytes, partial Rust staging rejection, source/test phase separation, affected-response parsing, repository-domain union when CodeGraph returns candidates, cross-language selection records, Cargo owner/downstream mapping, workspace fallback logic, and exact pre-commit target order.

## Pre-push verification and build

The pre-push hook reads every ref update supplied by Git and resolves the unique local commit tips that will be sent. Deleted refs need no verification. When Git supplies no ref updates, the hook skips verification and build without setting up a worktree. For each remaining unique tree, reuse a stable isolated checkout of that exact commit and run:

```text
make verify
make build
```

Run `make build` only if `make verify` succeeds. A verification failure must prevent the build from starting. A build failure must abort the push.

Do not verify or build a dirty working tree and assume it represents the pushed commit. Reuse safe build caches, but keep source, generated outputs, build artifacts, and reports tied to the commit SHA. Dependencies and tool versions remain locked. A missing tool or dependency fails the hook rather than silently skipping a gate.

The hook serializes access with a kernel lock at `.cache/gates/pre-push.lock` and reuses one ignored worktree at `.cache/gates/pre-push-worktree`. Before verifying each commit, it discards changes and untracked outputs from the prior run, then checks out the exact commit in detached mode. The stable source root preserves valid Cargo fingerprints between pushes; the lock prevents concurrent hooks from mutating that checkout.

Make's default test cache and the pre-push shared cache resolve existing
symlinks to their real paths, avoiding extra worktree prefixes in Unix sockets.

Fail fast from cheapest to costliest: check all formatting and static lint or
environment definitions before compiler-backed Rust diagnostics, then run
fast in-process tests before oracle, simulator, and virtual-system tests.

`make verify` is the complete non-release suite defined by the Makefile policy. It includes formatting, compiler checks, Clippy, rustdoc, ast-grep, rule tests, unit tests, integration tests, oracle checks, fixture checks, packaging checks, and every reproducible simulator or virtual-system check described by the connection-test policy. Checks that need Linux capabilities or virtual radios must run non-interactively through a prepared local VM, container, or namespace. Physical-phone checks are manual only. Hooks, `make verify`, and `make build` must never attempt them.

The hook stops on the first failed child gate and aborts the push. It writes `.cache/gates/pre-push-<sha>.json` with the commit SHA, aggregate durations and statuses for `make verify` and `make build`, and artifact paths. Gate failures also overwrite the record. The record does not cache successful verification or measure individual child gates. A successful result is the final automated quality decision before GitHub receives the commit. After it passes, push normally to the approved GitHub remote. Never force-push as part of this workflow.

## Gate documentation

Creating, renaming, or splitting a gate updates [Fast feedback gates](#fast-feedback-gates) in this document in the same change. Give the exact Make or Cargo command and its scope in no more than two lines.

The initial hook change must include contract fixtures for staged-only behavior, mirror/index setup and reuse, partial staging rejection, additions, deletions, renames, the first commit, CodeGraph stale and error fallbacks, LSP fallbacks, dependent-only candidate selection, Rust test-path recognition, unit and doc-test fallback, Cargo target mapping, multiple pushed refs, locked stable exact-commit worktree reuse, semantic commit validation, hook failure propagation, verification failure preventing a build, and build failure preventing a push.

## Fast feedback gates

- `make format-app-check` checks Rust; `make format-tooling-check` checks
  tooling and repository configuration.
- `make format-docs-check` checks Markdown; `make format-check` combines all
  formatting domains.
- `make lint-rust-clippy` runs strict Clippy; `make lint-rust-docs` fails
  rustdoc warnings; `make lint-rust-analyzer` runs rust-analyzer diagnostics.
  `make lint-rust` runs those three child gates.
- `make ruff-provision` installs pinned Ruff; `make analyzers-provision`
  installs or validates every pinned cross-language analyzer.
- `make lint-python` checks all Python tooling; `make lint-javascript` runs
  every current non-deprecated ESLint core rule as an error.
- `make lint-analysis-general` runs strict jscpd, cargo-machete, Knip, Ruff, and
  Vulture checks.
- `make lint-analysis-clang-tidy` and `make lint-analysis-cppcheck` isolate
  strict native analysis; `make lint-analysis` combines all three groups.
- `make lint-ast` runs the complete error-only ast-grep scan;
  `make test-ast-rules` checks its rule fixtures and snapshots.
- `make lint-docs` checks Markdown policy; `make lint-structure-app` and
  `make lint-structure-tooling` isolate structure feedback.
  `make lint-structure` combines them.
- `make pre-commit-source-{format,lint,ast,analysis}` checks exact staged
  non-test files with applicable tools, in that order.
- `make pre-commit-test-{format,lint,ast,analysis}` checks exact staged test
  files with applicable tools, in that order.
- `make pre-commit-domain-analysis` reruns applicable analyzers across every
  complete repository domain touched by the staged change.
- `make pre-commit-test` runs all selected tests through the library,
  application, and tooling children, serially and fail-fast in that order.
- `make pre-commit-test-libraries` runs selected Rust library/shared-contract
  packages outside `crates/app`, including integration-only suites, within 60s.
- `make pre-commit-test-app` runs the selected `crates/app` package's tests,
  including its library target and doc tests, within 60s.
- `make pre-commit-test-tooling` runs selected runnable Node tests within 60s.
- `make lint-android` validates Android SDK, probe, and AVD pins; `make android-preflight` checks host and KVM support.
- `make android-bootstrap` fetches pinned host tools; `make android-orchestrator-provision` prepares the pinned Mobly controller.
- After `make android-license`, `make android-provision` prepares the SDK, probe, and AVDs; `make android-seed` records clean first boots.
- `make android-up` starts and checks the prepared peers; `make android-down` stops them and records lifecycle time.
- `make lint-sources` validates immutable test-source pins; `make test-source-cache` hash-checks their prepared archives.
- `make sources-fetch` provisions the pinned source cache; provisioning is not part of a child test's 60-second execution budget.
- `make lint-oracle` checks the pinned oracle definition; `make oracle-provision` builds it outside the test budget.
- `make oracle-up` and `make oracle-down` measure lifecycle time; `make test-oracle-toolchain` tests the warm environment.
- `make proxy-up` and `make proxy-down` measure proxy lifecycle time; `make test-proxy-toxiproxy` checks TCP cutoff and recovery in both directions.
- `make dbus-up` and `make dbus-down` measure private-bus lifecycle; `make test-dbus-bluez` and `make test-dbus-networkmanager` check service templates through real clients.
- `make lint-bluetooth-radio` checks the pinned real-radio definition; `make bluetooth-radio-provision` builds it outside test time.
- `make bluetooth-radio-{up,down}` measures lifecycle; `make test-bluetooth-{controller,ble,classic}` checks isolated BlueZ radio paths.
- `make lint-live-bwu-kvm` checks the two-guest KVM harness;
  `make live-bwu-kvm-provision` builds its sealed Google-peer image.
- `make test-live-bwu-kvm` proves isolated LAN and Classic byte paths.
- `make network-up` and `make network-down` measure virtual-radio lifecycle; `make test-network-wmediumd` and `make test-network-netem` check 802.11 and UDP fault recovery.
- `make test-network-lan`, `make test-network-hotspot-client`, and `make test-network-hotspot-owner` check real Wi-Fi association and bidirectional TCP paths.
- `make test-network-wifi-direct-client` checks the supported Linux-client P2P role against a simulated remote group owner.
- `make oracle-reference-provision` builds the pinned Google oracle; `make test-oracle-reference` checks UKEY2 both ways.
- `make test-oracle-{bluetooth,ble,lan,hotspot,wifi-direct}` checks one pinned Google simulated connection family.
- `make test-oracle-bwu-handler` checks selected Bluetooth, Wi-Fi Direct, and LAN simulated semantics; `make test-oracle-bwu-fallback` checks selected fallback semantics, not cross-peer transfer interoperability.
- `make test-rust` runs workspace Rust tests; `make test-tooling` runs
  quality-gate and hook contract tests.
- `make test-plugin-release` checks the allowlisted plugin export and every
  native availability state through Quick Shell.
- `make test-local-install` checks local binary installation and the systemd
  user-service lifecycle; `make install-local` performs the local install.
- `make test-contracts` runs shared transfer scenarios against fast doubles; simulator adapters consume the same scenarios.
- `make test-nearshare-reference` runs the pinned diverse LAN peer through discovery, encryption, both transfer roles, and byte-integrity checks.
- `make diverse-lan-{up,down}` measures the isolated NearShare↔Google-derived LAN lifecycle.
- `make test-diverse-lan` checks simulated/reference mDNS, PIN fingerprints, both transfer roles, SHA-256 bytes, and a clean repeat.
- `make rust-lan-provision` rebuilds the daemon image; `make test-rust-lan-{outbound,inbound}` checks encrypted FILE bytes in one direction each.
- `make test-rust-lan-{inbound-content,outbound-content}` checks text and the exact 20-byte URL with decoded Retry 12 in one direction each.
- `make test-rust-lan-rejection` checks both receiver roles while held before consent and payload data; `make test-rust-lan-cancellation-{inbound,outbound}` gives each sender role its own 60-second child.
- `make test-rust-lan-retry` checks Retry 12 plus FILE data in both roles; `make test-rust-lan-failure-{inbound,outbound}` gives each pre-consent/data peer-loss role its own 60-second child.
- `make test-rust-lan` provisions once, then runs all ten LAN child gates; use it before claiming live LAN interoperability.
- `make lint-nearby-linux` and `make test-nearby-linux-tooling` check the
  Google-derived peer definition and its fast contract tests.
- `make test-nearby-linux-connections`, `make test-nearby-linux-sharing`, and
  `make test-nearby-linux-sharing-actions` run each prepared live peer suite.
- `make test-nearby-linux-sharing-fixtures` compares every generated Google
  Sharing frame and trace with the pinned corpus.
- `make test-android-nearby` runs the experimental AVD admission control; it is not a compatibility gate until it passes repeatably.
- `make verify-app` gives application-only feedback; `make verify-tooling` checks development tooling without starting environments.
- `make pre-commit` checks the staged snapshot and its affected tests;
  `make pre-push` serializes exact-commit verification and builds in a stable
  ignored worktree.
- `make verify` runs the complete local quality suite; `make build` performs the locked workspace build after verification.

## Deferred setup

This document authorizes policy, not setup. Do not create Git hooks, the staged-tree mirror, its CodeGraph index, install rust-analyzer, initialize Git, configure a GitHub remote, commit, or push until project setup begins explicitly.
