# Development, commit, and push workflow

Policy date: 2026-09-02

## Project rule

Build every behavior change with test-driven development through a confirmed seam. Commit each working vertical slice as soon as its targeted gates pass. Every commit follows Conventional Commits. Before a commit, hooks format and lint the staged change, then test the wider code area affected by it. Before a push, hooks run the complete local quality suite against the exact commit being pushed, then build that commit.

Local hooks are the project's automated verification. The pre-commit hook gives targeted feedback. The pre-push hook is authoritative. It verifies and then builds the exact commit being pushed. Hosted CI and automated release builds are outside the current scope. A future decision may add release automation that calls the same local targets, but it must not replace local verification or the source-build fallback.

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

## Staged snapshot

The pre-commit input is the Git index, not every working-tree change. Collect staged additions, copies, modifications, renames, and type changes relative to `HEAD`. For the first commit, compare against Git's empty tree.

Run checks against an ignored staged-tree mirror at `.cache/gates/pre-commit-tree/`. Its stable path lets CodeGraph reuse one database instead of rebuilding an index in a new temporary directory for every commit. Never run a mutating formatter against the developer's working tree from a hook. If a staged Rust file also has unstaged edits, fail before analysis. CodeGraph and rust-analyzer must analyze the same staged tree that will enter the commit.

`make hooks-install` creates the mirror from `HEAD`, or Git's empty tree before the first commit, and performs its initial full CodeGraph index. At the start of each pre-commit run, the hook refreshes the mirror's tracked files from the exact staged snapshot while preserving its local `.codegraph/` database, then runs `codegraph sync --quiet` inside the mirror. Routine pre-commit runs never perform a full re-index. A missing, corrupt, or incompatible mirror index fails with the exact setup command needed to rebuild it.

The hook checks that the mirror tree matches the Git index before trusting analysis. CodeGraph and rust-analyzer run only inside that mirror. The dirty working tree and the repository root's developer-facing CodeGraph index are not pre-commit inputs.

Deleted files remain inputs to impact analysis even though formatting and linting cannot read them. Renames contribute both the old and new paths when dependency selection needs them.

## Pre-commit gate order

The top-level `make pre-commit` command is an aggregate and may exceed one minute. Each child test gate remains directly runnable, is listed in `make help`, and has the existing 60-second execution budget. Prepared-environment lifecycle time is measured separately under the connection-test policy.

Run child gates in this fail-fast order:

1. Validate the staged snapshot, hook configuration, and changed-file list.
2. Refresh the staged-tree mirror, incrementally sync its CodeGraph index, and prove that the mirror matches the Git index.
3. Check file-size limits for changed project-authored files.
4. Check formatting for changed files with their pinned formatter.
5. Run ast-grep against changed Rust files with every applicable rule at error severity.
6. Run rust-analyzer diagnostics for changed Rust files.
7. Run compiler and Clippy checks for the smallest Cargo targets that own or consume those files.
8. Run directly changed tests and every test target selected by impact analysis.

The hook prints each selected command before running it. On failure it prints the narrow Make target that reproduces the result. It also records the changed files, Cargo owners, selected tests, selection source, fallback reason, and gate timings.

### Formatting and linting granularity

Rustfmt can check staged `.rs` files directly in the staged-tree mirror. Cargo's formatter can select packages but does not expose a changed-file selector; see the official [`cargo fmt` options](https://doc.rust-lang.org/cargo/commands/cargo-fmt.html).

Clippy and rustc analyze compilation units rather than independent source files. Map a changed file to the smallest valid Cargo target:

- a library-only source change selects that package's library target
- a binary-only source change selects that binary target
- an integration-test change selects that test target
- a shared module selects every target that compiles it
- `Cargo.toml`, `Cargo.lock`, `build.rs`, features, or shared generated inputs select every target in the affected package

Use the complete lint flags from the Rust lint policy for every selected target. Do not replace Clippy with LSP diagnostics. Clippy remains the compiler-backed lint result, as described in the [official Clippy usage guide](https://doc.rust-lang.org/clippy/usage.html).

## Affected-test selection

Staged files define the change, not the test boundary. A project-owned selector collects staged paths and Git statuses, queries analysis tools, maps the result to Cargo, and invokes the tests. CodeGraph supplies graph data to that selector. It is not the selector or test runner.

Start with every staged test file. For staged production files, take the union of these inputs:

1. Run `codegraph affected --json` inside the synced staged-tree mirror. It walks transitive dependents from each changed path and returns candidate test-file paths. Set the traversal depth explicitly and prove it with repository fixture graphs rather than relying on CodeGraph's default depth of five.
2. rust-analyzer identifies the changed symbols and follows references and incoming calls, including references represented through Rust macro expansion where supported.
3. Cargo metadata maps changed files and candidate test files to owning packages, downstream packages, and addressable library, binary, integration-test, or documentation-test targets.

The installed CodeGraph 1.6.0 implementation walks dependent files through resolved cross-file symbol edges. It does not walk dependencies, inspect Git, refresh its database, understand Cargo targets, or run tests. Its default path matcher also misses a relative path beginning with `tests/`. The project selector supplies an explicit Rust test-path filter and verifies it against root and nested Cargo integration-test fixtures. See [CodeGraph affected-test semantics](research/codegraph-affected-semantics.md) for the pinned behavior and sources.

Rust-analyzer's [Find All References](https://rust-analyzer.github.io/book/features.html#find-all-references) supplies compiler-aware symbol references. Standard LSP references and incoming-call requests are defined by the [Language Server Protocol](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).

Use a union, never an intersection. One input finding a test is enough to run it. CodeGraph widens the candidate set through stored symbol edges. Rust-analyzer contributes name resolution and macro-aware references. Cargo owns the final package and target scope. None of them proves runtime behavior.

CodeGraph cannot name unit tests embedded in production `.rs` files or Rust doc tests because neither has a separate test-file path. Every production change therefore runs the owning package's unit and doc tests even when CodeGraph returns integration-test candidates. The selector maps every candidate path through Cargo metadata before invoking Cargo; it never treats a path or filename as a runnable target by itself.

Fall back conservatively:

- If the staged-tree mirror or its index is missing, corrupt, or incompatible, stop with the setup command that recreates it.
- If CodeGraph is stale, does not match the staged tree, reports a query or sync error, or cannot classify a changed file, run all tests for every owning and downstream package and record the fallback reason.
- If rust-analyzer is absent, unhealthy, times out, or cannot resolve a changed symbol, run all tests for every owning package.
- If no test is selected for a production change, run the owning package's full tests. An empty selection never means "nothing to test."
- For a deletion or rename, use the old and new paths as selector inputs, then run all owning and downstream package tests when the after-snapshot cannot prove the old path's impact.
- For an unmapped path, public seam, shared protocol type, workspace manifest, lockfile, feature graph, build script, toolchain file, or test selector change, run all downstream package tests.
- For a hook, impact-selector, or gate change, run its contract fixtures and the full local suite before commit.

Use Cargo's package and target selectors to run the result, such as `--lib`, `--bin`, `--test`, and `--doc`. The [Cargo test reference](https://doc.rust-lang.org/cargo/commands/cargo-test.html) defines these addressable units. If the selected unit still exceeds 60 seconds, split it at a real test responsibility and document the new gate in `AGENTS.md`.

Before application development starts, the hook setup must pin CodeGraph and rust-analyzer, build the staged-tree mirror and index, and prove affected-test selection against fixture repositories. The fixtures must cover mirror reuse and freshness, direct and transitive dependents, downstream packages, root and nested integration tests, embedded unit tests, doc tests, partial staging rejection, renamed and deleted files, changed tests, candidate-to-Cargo mapping, empty results, stale indexes, and tool failure.

## Pre-push verification and build

The pre-push hook reads every ref update supplied by Git and resolves the unique local commit tips that will be sent. Deleted refs need no verification. For each remaining unique tree, create an isolated checkout of that exact commit and run:

```text
make verify
make build
```

Run `make build` only if `make verify` succeeds. A verification failure must prevent the build from starting. A build failure must abort the push.

Do not verify or build a dirty working tree and assume it represents the pushed commit. Reuse safe build caches, but keep source, generated outputs, build artifacts, and reports tied to the commit SHA. Dependencies and tool versions remain locked. A missing tool or dependency fails the hook rather than silently skipping a gate.

`make verify` is the complete non-release suite defined by the Makefile policy. It includes formatting, compiler checks, Clippy, rustdoc, ast-grep, rule tests, unit tests, integration tests, oracle checks, fixture checks, packaging checks, and every reproducible simulator or virtual-system check described by the connection-test policy. Checks that need Linux capabilities or virtual radios must run non-interactively through a prepared local VM, container, or namespace. Physical-phone checks are manual only. Hooks, `make verify`, and `make build` must never attempt them.

The hook stops on the first failed child gate and aborts the push. A successful result records the verified and built commit SHA, gate timings, build timing, and artifact paths. That local result is the final automated quality decision before GitHub receives the commit. After it passes, push normally to the approved GitHub remote. Never force-push as part of this workflow.

## Gate documentation

Creating, renaming, or splitting any hook child gate updates the root `AGENTS.md` in the same commit. Its `Fast feedback gates` section gives the exact Make command and the feature or failure class it checks in no more than two lines.

The initial hook change must include contract fixtures for staged-only behavior, mirror/index setup and reuse, partial staging rejection, additions, deletions, renames, the first commit, CodeGraph stale and error fallbacks, LSP fallbacks, dependent-only candidate selection, Rust test-path recognition, unit and doc-test fallback, Cargo target mapping, multiple pushed refs, an exact-commit checkout, semantic commit validation, hook failure propagation, verification failure preventing a build, and build failure preventing a push.

## Deferred setup

This document authorizes policy, not setup. Do not create Git hooks, the staged-tree mirror, its CodeGraph index, install rust-analyzer, initialize Git, configure a GitHub remote, commit, or push until project setup begins explicitly.
