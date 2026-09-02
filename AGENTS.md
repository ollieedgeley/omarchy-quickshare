# Repository instructions

## Current authority

Repository setup, quality tooling, local hooks, commits, and pushes are authorized. Application behavior remains out of scope until the verification-environment start condition in the programmatic connection-testing document is satisfied.

## Work loop

1. Read the closest `AGENTS.md` and the task-specific document listed below.
2. Inspect the current tree and preserve user changes. A planned path or command does not exist until the repository contains it.
3. Use CodeGraph before direct code search or file reads. Use `rg` or `rg --files` for prose and unindexed content.
4. Make the smallest complete change. Behavior changes follow the TDD loop below.
5. Run the narrowest relevant gate that exists. Fix failures before broadening the check.
6. Finish with the requested result, relevant checks passing, and affected policy or command documentation updated.

## Read before changing

- Domain terms or names: read [CONTEXT.md](CONTEXT.md) and use its vocabulary.
- Crates, modules, dependencies, tests, fixtures, tools, packaging, or paths: read [project structure](docs/architecture/project-structure.md) and preserve its ownership and dependency map.
- Product behavior, compatibility claims, protocol coverage, dependencies, licensing, or implementation strategy: read [Quick Share feasibility](docs/quick-share-feasibility.md) and [Rust feasibility](docs/rust-reimplementation-feasibility.md).
- Connection seams, simulators, oracles, virtual systems, test support, or coverage claims: read [programmatic connection testing](docs/connection-mocking-tools.md).
- Application behavior, gates, hooks, affected-test selection, commits, or pushes: read [development workflow](docs/development-workflow.md).
- ast-grep configuration, rules, suppressions, scans, or rule tests: read [strict ast-grep policy](docs/ast-grep-strict-rust-policy.md).
- Workspace shape or source-versus-plugin distribution: read [ADR 0001](docs/adr/0001-workspace-and-distribution-shape.md).

Keep detailed policy in its owning document. Do not copy it into this file.

<!-- CODEGRAPH_START -->

## CodeGraph

Use CodeGraph before grep, find, or direct file reads when locating or understanding code:

- Prefer the `codegraph_explore` MCP tool when available.
- The shell fallback is `codegraph explore "<symbols or question>"`.

Ask for named files or symbols when source is deferred. Treat returned source as already read and inspect its callers, callees, and dependents before editing. Use LSP references as well when changing an exported symbol.
<!-- CODEGRAPH_END -->

## Repository handling

- Follow the ownership, dependency, file-count, and directory-count rules in the project structure before adding or moving files.
- Count physical lines before finishing. Project-authored files and every `AGENTS.md` stop at 500 lines; tests and test-only support stop at 800.
- Before writing application code, satisfy the verification-environment start condition in the programmatic connection-testing document.

## TDD and test design

- For each behavior, name the observable seam, add the smallest failing test, confirm the intended failure, write only enough code to pass, refactor while green, and rerun the targeted gates.
- Commit a behavior's test, implementation, fixture, and direct support changes together. Documentation, build, and test-infrastructure work needs tests only where it creates an executable contract.
- Use the test doubles and simulator hierarchy defined in the programmatic connection-testing document. Keep test time, randomness, synchronization, and failure injection deterministic.
- Test public behavior and observable state. Keep repeated setup in honestly named helpers, builders, factories, fixtures, fakes, stubs, or mocks.

## Quality gates

- The root `Makefile` is the public task interface; Cargo remains the Rust build system. Release users do not need Make.
- Gates fail on the first unhandled error. Each directly runnable child test gate has a 60-second execution limit. Prepared-environment startup and teardown use separate measured lifecycle targets, aim for 30 seconds, and should not exceed 60 seconds where practical; their time is not charged to the test gate.
- Split a slow child by a real responsibility, suite, connection type, or environment. A wrapper, sibling aggregate, or background process does not reset the limit.
- Pin Rust, rustfmt, Clippy, rust-analyzer, ast-grep, and other verification tools. Missing tools fail a gate instead of silently reducing coverage.
- Treat formatting differences and every enabled diagnostic as errors. Use the lint and ast-grep policies for exact configuration and exceptions.
- Creating, renaming, or splitting a gate updates `Fast feedback gates` below in the same change. Give the exact Make or Cargo command and its scope in no more than two lines.

## Git workflow

- Use the tracked Husky hooks and the Conventional Commit types defined in the development workflow. Do not add hosted CI; local verification is authoritative.
- Pre-commit checks the staged snapshot and the wider affected test set selected by the development workflow. Treat CodeGraph output as candidate data, not the final test scope.
- Pre-push verifies the exact commit with `make verify`, then runs `make build` only after verification passes. Neither command may require a physical phone.
- During authorized implementation, commit each green vertical slice after targeted gates pass, then push through the pre-push hook to the approved remote.
- Never use `--no-verify`, disabled tests, red or `WIP` commits, or force pushes.

## Fast feedback gates

- `make format-app-check` checks Rust; `make format-tooling-check` checks JavaScript and repository documents. `make format-check` combines both.
- `make lint-rust` runs compiler, rustdoc, rust-analyzer, and all enabled Clippy diagnostics as errors.
- `make lint-javascript` runs every current non-deprecated ESLint core rule as an error with no inline overrides.
- `make lint-ast` runs the complete error-only ast-grep scan; `make test-ast-rules` checks its rule fixtures and snapshots.
- `make lint-docs` checks Markdown policy; `make lint-structure-app` and `make lint-structure-tooling` isolate structure feedback. `make lint-structure` combines them.
- `make lint-android` validates Android SDK, probe, and AVD pins; `make android-preflight` checks host and KVM support.
- `make lint-sources` validates immutable test-source pins; `make test-source-cache` hash-checks their prepared archives.
- `make sources-fetch` provisions the pinned source cache; provisioning is not part of a child test's 60-second execution budget.
- `make lint-oracle` checks the pinned oracle definition; `make oracle-provision` builds it outside the test budget.
- `make oracle-up` and `make oracle-down` measure lifecycle time; `make test-oracle-toolchain` tests the warm environment.
- `make proxy-up` and `make proxy-down` measure proxy lifecycle time; `make test-proxy-toxiproxy` checks TCP cutoff and recovery in both directions.
- `make dbus-up` and `make dbus-down` measure private-bus lifecycle; `make test-dbus-bluez` and `make test-dbus-networkmanager` check service templates through real clients.
- `make lint-bluetooth-radio` checks the pinned real-radio definition; `make bluetooth-radio-provision` builds it outside test time.
- `make bluetooth-radio-{up,down}` measures lifecycle; `make test-bluetooth-{controller,ble,classic}` checks isolated BlueZ radio paths.
- `make network-up` and `make network-down` measure virtual-radio lifecycle; `make test-network-wmediumd` and `make test-network-netem` check 802.11 and UDP fault recovery.
- `make test-network-lan`, `make test-network-hotspot-client`, and `make test-network-hotspot-owner` check real Wi-Fi association and bidirectional TCP paths.
- `make test-network-wifi-direct-client` checks the supported Linux-client P2P role against a simulated remote group owner.
- `make oracle-reference-provision` builds the pinned Google oracle; `make test-oracle-reference` checks UKEY2 both ways.
- `make test-oracle-{bluetooth,ble,lan,hotspot,wifi-direct}` checks one pinned Google simulated connection family.
- `make test-rust` runs workspace Rust tests; `make test-tooling` runs quality-gate and hook contract tests.
- `make verify-app` gives application-only feedback; `make verify-tooling` checks development tooling without starting environments.
- `make pre-commit` checks the staged snapshot and its affected tests; `make pre-push` verifies and builds each pushed commit.
- `make verify` runs the complete local quality suite; `make build` performs the locked workspace build after verification.

## Maintaining this guide

- Keep this file concise and broadly applicable. Prefer fewer than 200 lines; 500 is the hard limit.
- Add a rule after a repeated agent error or when a project fact cannot be inferred from code, commands, or the linked source of truth.
- Put a subsystem-only rule in a nested `AGENTS.md` when that subtree exists. Keep essential repository policy here and do not duplicate parent instructions.
- Update or remove stale instructions in the same change that alters the related workflow, command, path, or policy.

## SKILLS

ponytail
ponytail review
implement
tdd

Use other skills if they are applicable.
Find them here:
/home/ollie/Skills
