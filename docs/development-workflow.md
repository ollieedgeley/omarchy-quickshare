# Development, commit, and push workflow

Policy date: 2026-09-02

## Project rule

Build every behavior change with test-driven development through a confirmed seam. Commit each working vertical slice as soon as its targeted gates pass. Every commit follows Conventional Commits. Before a commit, hooks format and lint the staged change, then test the wider code area affected by it. Before a push, hooks run the complete local quality suite against the exact commit being pushed, then build that commit.

Local hooks are the project's automated verification. The pre-commit hook gives targeted feedback. The pre-push hook is authoritative. It verifies and then builds the exact commit being pushed. Hosted CI and automated release builds are outside the current scope. A future decision may add release automation that calls the same local targets, but it must not replace local verification or the source-build fallback.

Application and development-tool feedback have separate aggregates. `make verify-app` runs only Rust formatting, compiler checks, Rust diagnostics, ast-grep, and Rust tests. `make verify-tooling` runs the separate tooling and documentation formatting checks, every current ESLint core rule, static environment definitions, and fast tooling contracts; it does not start simulators or virtual devices. `make verify` combines both with every programmatic environment check. This keeps environment implementation out of the normal application loop without weakening pre-push verification.

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

Normal development must not use `--no-verify`. It bypasses the project's only automated verification before GitHub receives the change.

## OMP per-edit policy

`lsp.formatOnWrite` is disabled. The post-edit hook runs check-only exact-file formatting and lint feedback and returns a tool error on failure; it never rewrites files after OMP snapshots.

## Staged snapshot

Staged quality paths are the added, copied, modified, renamed, or type-changed entries whose after-snapshot exists in the index. Staged source files are the non-test members of that set. Staged tests are directly staged test files. Extended tests are additional test endpoints selected by impact analysis.

`make hooks-install` creates the mirror from `HEAD`, or Git's empty tree before the first commit, and performs its initial full CodeGraph index. At the start of each pre-commit run, the hook refreshes the mirror's tracked files from the exact staged snapshot while preserving its local `.codegraph/` database, then runs `codegraph sync --quiet` inside the mirror. Routine pre-commit runs never perform a full re-index. A missing, corrupt, or incompatible mirror index fails with the exact setup command needed to rebuild it.
The hook checks that the mirror tree matches the Git index before trusting analysis. CodeGraph and compiler-backed staged gates run only inside that mirror. The dirty working tree and the repository root's developer-facing CodeGraph index are not pre-commit inputs.

Deleted files remain impact-analysis inputs even though exact-file formatting and linting cannot read them. Renames contribute both old and new paths.

## Pre-commit gate order

The top-level `make pre-commit` command is an aggregate and may exceed one minute. Each child test gate remains directly runnable, is listed in `make help`, and has the existing 60-second execution budget. Prepared-environment lifecycle time is measured separately under the connection-test policy.

The public pre-commit sequence is prepare, structure, source-format/lint/ast, test-format/lint/ast, then affected tests.

Run child gates in this fail-fast order:

1. Prepare (staged snapshot validation, mirror refresh, CodeGraph sync, index match proof).
2. Structure (file-size limits for changed project-authored files).
3. Source gates: `pre-commit-source-format` (format-source), `pre-commit-source-lint` (lint-source), `pre-commit-source-ast` (ast-source).
4. Staged-test gates: `pre-commit-test-format` (format-tests), `pre-commit-test-lint` (lint-tests), `pre-commit-test-ast` (ast-tests).
5. Affected tests via `pre-commit-test` (test) using `computeSelectionRecord` (records `{path,language,domain}` in stagedSources/stagedTests/extendedTests), `parseAffectedJson` (only affected test paths), `packageSelection` (Rust owner/downstream packages).

Behavior development begins with the smallest in-process test at an external
seam. Deterministic fakes, stubs, and mocks provide routine feedback, while the
same semantic scenario is retained for the relevant adapter contract and
simulator or oracle suite. Broaden to those slower layers after the behavior is
green; use their results to correct a drifting test double rather than weakening
the shared scenario.

The hook prints each selected command before running it. It records staged sources, staged tests, extended tests, CodeGraph inputs and candidates, traversal count, fallback reason, selected Cargo packages, languages, domains, and runnable Node test paths in `.cache/gates/pre-commit-selection.json`.

### Formatting and linting granularity

Rustfmt can check staged `.rs` files directly in the staged-tree mirror. Cargo's formatter can select packages but does not expose a changed-file selector; see the official [`cargo fmt` options](https://doc.rust-lang.org/cargo/commands/cargo-fmt.html).

Prettier receives only changed files in the formats it supports. ESLint receives only changed, existing JavaScript or TypeScript paths from the staged mirror. Ruff receives only changed Python paths. These checks run separately for staged sources and staged tests; they never widen to unrelated files. Tests are selected separately through CodeGraph and domain ownership.

Cargo check, Clippy, and rustdoc operate on packages rather than independent source files. `packageSelection` maps changed Rust paths to their owning workspace packages and transitive downstream workspace packages. Shared workspace inputs or Rust paths without an owner conservatively select every workspace package. `rust-lints.mjs` receives all selected `--package` arguments in one invocation. Rust-analyzer's diagnostics command has no stable package selector, so package-scoped pre-commit runs omit it; the workspace-wide `make lint-rust` gate retains rust-analyzer diagnostics.

Use the complete lint flags from the Rust lint policy for every selected package. Do not replace Clippy with LSP diagnostics. Clippy remains the compiler-backed lint result, as described in the [official Clippy usage guide](https://doc.rust-lang.org/clippy/usage.html).

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

The pre-push hook reads every ref update supplied by Git and resolves the unique local commit tips that will be sent. Deleted refs need no verification. For each remaining unique tree, create an isolated checkout of that exact commit and run:

```text
make verify
make build
```

Run `make build` only if `make verify` succeeds. A verification failure must prevent the build from starting. A build failure must abort the push.

Do not verify or build a dirty working tree and assume it represents the pushed commit. Reuse safe build caches, but keep source, generated outputs, build artifacts, and reports tied to the commit SHA. Dependencies and tool versions remain locked. A missing tool or dependency fails the hook rather than silently skipping a gate.
Fail fast from cheapest to costliest: check all formatting and static lint or
environment definitions before compiler-backed Rust diagnostics, then run
fast in-process tests before oracle, simulator, and virtual-system tests.

`make verify` is the complete non-release suite defined by the Makefile policy. It includes formatting, compiler checks, Clippy, rustdoc, ast-grep, rule tests, unit tests, integration tests, oracle checks, fixture checks, packaging checks, and every reproducible simulator or virtual-system check described by the connection-test policy. Checks that need Linux capabilities or virtual radios must run non-interactively through a prepared local VM, container, or namespace. Physical-phone checks are manual only. Hooks, `make verify`, and `make build` must never attempt them.

The hook stops on the first failed child gate and aborts the push. A successful result records the verified and built commit SHA, gate timings, build timing, and artifact paths. That local result is the final automated quality decision before GitHub receives the commit. After it passes, push normally to the approved GitHub remote. Never force-push as part of this workflow.

## Gate documentation

Creating, renaming, or splitting any hook child gate updates the root `AGENTS.md` in the same commit. Its `Fast feedback gates` section gives the exact Make command and the feature or failure class it checks in no more than two lines.

The initial hook change must include contract fixtures for staged-only behavior, mirror/index setup and reuse, partial staging rejection, additions, deletions, renames, the first commit, CodeGraph stale and error fallbacks, LSP fallbacks, dependent-only candidate selection, Rust test-path recognition, unit and doc-test fallback, Cargo target mapping, multiple pushed refs, an exact-commit checkout, semantic commit validation, hook failure propagation, verification failure preventing a build, and build failure preventing a push.

## Deferred setup

This document authorizes policy, not setup. Do not create Git hooks, the staged-tree mirror, its CodeGraph index, install rust-analyzer, initialize Git, configure a GitHub remote, commit, or push until project setup begins explicitly.
