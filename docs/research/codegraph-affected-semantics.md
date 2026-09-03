# CodeGraph affected-test semantics

Research cutoff: 2026-09-03

Installed version: CodeGraph 1.6.0, released 2026-08-26 and marked latest at the cutoff. See the [v1.6.0 release](https://github.com/colbymchenry/codegraph/releases/tag/v1.6.0).

## Conclusion

CodeGraph supplies graph information and candidate test-file paths. It does not inspect the Git staging area, understand Cargo test targets, choose the final safe Rust test scope, or run tests. A project-owned pre-commit program must do those jobs. CodeGraph runs against every indexed staged code path, but `affected` returns only test endpoints plus the traversal count; it does not expose the full intermediate extended-file set. The project selector supplies Git/index freshness, domains, runner mapping, and conservative fallbacks.

Version 1.6.0 does include a useful `codegraph affected` CLI command. It is narrower than our current wording suggests. Given changed file paths, it walks transitive file dependents and prints paths that match its test-file heuristic. The command is an input to test selection, not the test selector as a whole. Tests are selected by dependency or domain ownership even when their implementation language differs from the staged file (for example, a QML change can require JS tests).

## Documented behavior

### `explore` and `affected` have different jobs

The default MCP surface exposes `codegraph_explore`. It accepts prose, symbols, or file names and returns source, call paths, and a blast-radius summary. It is meant for agent context, not machine-readable hook decisions. `affected` is a separate CLI command. The official README lists only `explore` on the default MCP surface and documents `affected` under the CLI. See the [MCP and CLI reference](https://github.com/colbymchenry/codegraph/blob/v1.6.0/README.md#codegraph-affected).

`codegraph affected` accepts paths as arguments or newline-separated standard input. It emits decorated text, quiet paths, or JSON. The JSON contains normalized changed paths, affected test paths, and the number of dependents visited. It never invokes a test runner. The README's hook example captures the output and passes it to Vitest in a separate command. See the [`affected` implementation](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/bin/codegraph.ts#L2193-L2335) and [hook example](https://github.com/colbymchenry/codegraph/blob/v1.6.0/README.md#L553-L560).

### It traverses dependents only

For each input source file, `affected` performs breadth-first search through `getFileDependents`. The default maximum depth is five. It does not walk the source file's dependencies, and it stops expanding a branch when a path qualifies as a test file. A changed path that already qualifies as a test is returned directly. See the [breadth-first traversal](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/bin/codegraph.ts#L2273-L2306).

The current graph projection treats a file as a dependent when a symbol in that file has any cross-file edge into the changed file, except `contains`. Relevant edge kinds include `calls`, `imports`, `exports`, `extends`, `implements`, `references`, `type_of`, `returns`, `instantiates`, `overrides`, and `decorates`. See the [dependent-file query](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/db/queries.ts#L1885-L1929) and [edge-kind definition](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/types.ts#L50-L72).

The README says the command traces "import dependencies." That description is stale shorthand. The v1.6.0 source explicitly uses the broader resolved symbol graph because `imports` edges alone do not represent cross-file dependence in its storage model. See [`getFileDependents`](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/graph/queries.ts#L109-L143).

### Test recognition is path-based

The command does not inspect Rust attributes, Cargo metadata, test registrations, or test names. Its built-in test patterns recognize:

- `.spec.` and `.test.` in a filename;
- `/__tests__/`, `/test/`, `/tests/`, `/e2e/`, and `/spec/` within a path.

Supplying `--filter` replaces that built-in recognition with one glob-derived regular expression. The prior `--filter **/*.rs` guidance is incorrect: that filter makes the changed Rust source itself a terminal match and prevents traversal to dependents, so pre-commit uses no filter. See the [test-path classifier](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/bin/codegraph.ts#L2246-L2271).

- The default `/tests/` expression does not match a relative path beginning with `tests/`. Standard top-level Cargo integration tests therefore need an explicit filter or project wrapper.
- Even when a test path is returned, CodeGraph does not map it to a Cargo package or target invocation.

The first two points follow directly from Cargo's source layout and the path-only classifier. The third was reproduced against installed 1.6.0: `codegraph affected tests/public_api.rs --json` returned an empty list, while `--filter 'tests/*.rs'` returned the path. This matches the regular expressions in the source.

CodeGraph does parse Rust structure. It advertises `.rs` as fully supported, and its Rust extractor covers functions, trait declarations, structs, unions, enums, type aliases, `use` declarations, and calls. Its own tests cover cross-module struct construction, traits, and re-exports. That graph support does not make the test-file heuristic Cargo-aware. See the [Rust extractor](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/extraction/languages/rust.ts#L74-L170) and [Rust dependency tests](https://github.com/colbymchenry/codegraph/blob/v1.6.0/__tests__/extraction.test.ts#L9317-L9393).

### It does not read the Git staging area

`affected` reads file paths supplied through arguments or standard input. Its own code does not ask Git which paths are staged. The README pipes an external `git diff` into it, which leaves the choice of working-tree, commit, or staged diff to the caller. See the [input collection](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/bin/codegraph.ts#L2198-L2235).

The graph also does not represent the staged snapshot. Indexing reads each path from the working filesystem. `CodeGraph.open` syncs only when its caller requests `sync: true`, while `affected` opens the database without that option. It therefore relies on an already fresh index, usually maintained by the watcher. See the [filesystem read](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/extraction/index.ts#L1910-L1935), [`CodeGraph.open`](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/index.ts#L328-L363), and [`affected` open call](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/bin/codegraph.ts#L2242-L2244).

The following are inferences from that implementation:

- A partially staged file can have different edges in the working tree and the staged snapshot.
- An unsynced graph can return candidates based on older code.
- A deletion or rename can remove the old graph nodes needed to discover their former dependents.
- An empty result is not proof that no test can fail. It may mean no matching test path, no resolved edge, an unsupported file, stale data, or no indexed code.

The command exits successfully when it finds no tests. A hook must treat uncertainty and an empty candidate set as fallback conditions, not as permission to skip tests. See the [empty-result branch](https://github.com/colbymchenry/codegraph/blob/v1.6.0/src/bin/codegraph.ts#L2308-L2329).

## Required pre-commit design

These are project recommendations inferred from the facts above:

1. Let the hook collect staged paths. Use the Git index explicitly. Do not say CodeGraph finds staged changes.
2. Let a project-owned selector call the CodeGraph CLI and parse `--json`. Do not use natural-language `explore` output in a hook.
3. Check index freshness before trusting candidates. A failed, stale, or incomplete graph triggers a conservative Cargo fallback.
4. Treat CodeGraph output as one candidate set. Union it with changed tests, repository-domain ownership, and Cargo owner/downstream packages.
5. Run the tests in the hook. Map integration-test paths through Cargo metadata instead of assuming a filename is a target name.
6. Always cover Rust unit and doc tests at package scope because CodeGraph cannot name them as separate files.
7. Fall back for partial staging, deletions, renames, manifests, lockfiles, toolchain files, build configuration, unindexed paths, unresolved mappings, empty results, and tool errors.
8. Test the selector itself with fixture repositories. Cover direct and transitive dependents, embedded unit tests, doc tests, top-level integration tests, workspaces, partial staging, rename and deletion, stale indexes, empty output, and CodeGraph failure.

## Corrections to repository policy

The repository instructions should say:

- `codegraph affected` reports candidate test files reached through transitive dependents. It does not inspect staged content or run tests. `parseAffectedJson` returns only affected test paths; `computeSelectionRecord` returns records `{path,language,domain}` in `stagedSources`, `stagedTests`, `extendedTests`.
- The pre-commit selector supplies staged paths, checks graph freshness, combines CodeGraph with repository domains and Cargo metadata, chooses the safe test scope, and invokes the runners. `packageSelection` maps Rust paths to owner/downstream packages.
- Rust unit and doc tests require a package-level fallback. Standard `tests/` integration paths need project-owned recognition because the v1.6.0 default misses a path beginning with `tests/`.
- Empty, stale, failed, partially staged, deleted, renamed, or unmapped input broadens the gate. It never narrows it to zero.
- The hook uses the CLI `affected` command. `codegraph_explore` remains the agent-facing tool for understanding code and blast radius. `rust-lints.mjs --package NAME` narrows compiler-backed lints.
