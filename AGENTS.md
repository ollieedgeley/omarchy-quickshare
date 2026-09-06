# Repository instructions

## Scope and work loop

- Work within the agreed task and the user's current scope.
- Read the relevant policy below before changing its subject.
- Preserve user changes.
- Confirm planned paths and commands exist before using them.
- Make the smallest complete change.
- Finish with the requested result and relevant checks passing.

## Read before changing

- Domain terms or names: use the vocabulary in [CONTEXT.md](CONTEXT.md).
- Ownership, dependencies, file layout, workspace shape, or packaging: read [project structure](docs/architecture/project-structure.md).
- Product behavior, compatibility claims, or protocol coverage: read [Quick Share feasibility](docs/quick-share-feasibility.md) and [Rust feasibility](docs/rust-reimplementation-feasibility.md).
- Rust implementation strategy, dependencies, or licensing: read [Rust feasibility](docs/rust-reimplementation-feasibility.md) and the relevant constraints in [Quick Share feasibility](docs/quick-share-feasibility.md).
- Connection seams, simulators, oracles, virtual systems, test support, or coverage claims: read [programmatic connection testing](docs/connection-mocking-tools.md).
- Behavior development, gates, hooks, affected-test selection, commits, or pushes: follow [development workflow](docs/development-workflow.md).
- ast-grep configuration, rules, suppressions, scans, or rule tests: read [strict ast-grep policy](docs/ast-grep-strict-rust-policy.md).

- Keep detailed policy in its owning document.

<!-- CODEGRAPH_START -->

## CodeGraph

Use CodeGraph before grep, find, or direct file reads when locating or understanding code:

- Prefer the `codegraph_explore` MCP tool when available.
- The shell fallback is `codegraph explore "<symbols or question>"`.

Ask for named files or symbols when source is deferred. Treat returned source as already read and inspect its callers, callees, and dependents before editing. Use LSP references as well when changing an exported symbol.
<!-- CODEGRAPH_END -->

## Repository handling

- Preserve the project structure's ownership, dependency, file-count, and directory-count rules when adding or moving files.
- Count physical lines before finishing.
- Keep project-authored files and every `AGENTS.md` at most 500 lines.
- Keep tests and test-only support at most 800 lines.
- Satisfy the connection-testing development threshold before writing application behavior.
- Treat live reference-peer interoperability as release evidence, not a development prerequisite.

## TDD and test design

- Name each behavior's observable seam before adding its smallest failing test.
- Confirm the intended failure before writing only enough code to pass.
- Refactor while green, then rerun the targeted gates.
- Commit each behavior's test, implementation, fixtures, and direct support together.
- Test documentation, build, and test-infrastructure changes only where they create an executable contract.
- Use the connection-testing policy's test doubles and simulator hierarchy.
- Keep test time, randomness, synchronization, and failure injection deterministic.
- Test public behavior and observable state.
- Keep repeated setup in honestly named helpers, builders, factories, fixtures, fakes, stubs, or mocks.

## Quality gates

- Use the root `Makefile` as the public task interface.
- Keep Cargo as the Rust build system without requiring Make for release users.
- Stop gates at the first unhandled error.
- Limit each directly runnable child test gate to 60 seconds.
- Measure prepared-environment startup and teardown in separate lifecycle targets outside the test budget.
- Aim for 30-second lifecycle targets, not exceeding 60 seconds where practical.
- Split slow children by responsibility, suite, connection type, or environment.
- Never reset the time limit through a wrapper, sibling aggregate, or background process.
- Pin Rust, rustfmt, Clippy, rust-analyzer, ast-grep, and other verification tools.
- Fail gates on missing tools instead of reducing coverage.
- Treat formatting differences and every enabled diagnostic as errors.
- Use the lint and ast-grep policies for exact configuration and exceptions.
- Select existing commands from [Fast feedback gates](docs/development-workflow.md#fast-feedback-gates).
- Update that catalogue in the same change that creates, renames, or splits a gate.

## Git workflow

- Use tracked Husky hooks and the development workflow's Conventional Commit types.
- Keep local verification authoritative; hosted execution is limited to the W1 runner pilot in the development workflow.
- Verify the exact staged snapshot and the wider affected test set in pre-commit.
- Treat CodeGraph output as candidate data, not the final test scope.
- Verify the exact pushed commit with `make verify`, then `make build` only after verification passes.
- Keep physical phones out of automated gates.
- Run checks only for the current change to reproduce a failure, prove a fix, exercise changed behavior, or validate a modified gate.
- Choose the narrowest existing command that observes the relevant contract.
- Fix failures before broadening the check.
- Leave aggregate formatting, linting, analysis, verification, and build gates to Git hooks.
- Never manually invoke `make pre-commit`, `make pre-push`, `make verify`, or `make build`.
- During authorized implementation, commit each green vertical slice after targeted gates pass.
- Push each authorized slice through the pre-push hook to the approved remote.
- Never use `--no-verify`, disabled tests, red or `WIP` commits, or force pushes.

## Waiting for commands

- Run long commits, pushes, and checks as OMP-managed background jobs.
- Do independent work while they run; otherwise use `hub wait` for the job with `timeoutMs: 0`.
- Rely on completion notifications. Check progress only for a specific problem or a user request.
- Inspect the final result before reporting success or starting dependent work.

## Maintaining this guide

- Keep this guide concise and broadly applicable, preferably below 200 lines.
- Add rules for repeated agent errors or project facts absent from code, commands, and linked policy.
- Put subsystem-only rules in a nested `AGENTS.md` when that subtree exists.
- Keep essential repository policy here without duplicating parent instructions.
- Update affected policy and command documentation in the same change.
- Remove stale instructions when their workflow, command, path, or policy changes.
