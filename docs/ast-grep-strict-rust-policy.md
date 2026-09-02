# Strict ast-grep policy for Rust

Research date: 2026-09-02

## Decision

Use ast-grep as an error-only structural lint gate beside rustc and Clippy. It is valuable for project rules that depend on syntax and file location, especially architecture boundaries and deterministic-test rules. It does not replace compiler linting, type-aware analysis, or behavior tests.

The project must write its configuration and rules explicitly. Do not accept the scaffold produced by `ast-grep new` as policy, inherit an unpinned remote rule pack, or rely on default severities. Every enabled project rule has `severity: error`, a specific message, an explicit file scope, and committed positive, negative, and snapshot tests.

Pin ast-grep to exactly `0.45.3`, released on 2026-08-31 and current on the research date. Record the binary checksum for every distributed host target. Pin schemas and documentation to the same tag. A version update is a reviewed dependency change that reruns every rule test and the full scan. The release is identified by tag and commit `979c143` in the [official release record](https://github.com/ast-grep/ast-grep/releases/tag/0.45.3).

The machine used for this research has ast-grep 0.44.1 installed. Do not let that local installation choose project behavior. Setup must install or invoke the pinned project version.

This is the strictest defensible policy. Enabling a syntax rule with known false positives is not stricter. It teaches developers to distrust or suppress the gate.

## What is exhaustive in this document

The following sections are exhaustive for ast-grep 0.45.3:

- project configuration fields
- YAML lint-rule fields
- rule-object matchers
- the official Rust catalog
- severity levels, built-in suppression checks, and official rule-test facilities

The proposed project rules are not an exhaustive list of every rule that could ever be written. ast-grep's matchers compose recursively, so there is no finite shipped list of all possible Rust rules. The proposals are the complete starting backlog justified by the current project architecture. Add rules when a confirmed seam, defect, or repeated review finding gives them a precise contract.

## Complete project configuration surface

The root configuration accepts these fields. Only `ruleDirs` is required. Paths resolve relative to `sgconfig.yml`. See the [official `sgconfig.yml` reference](https://ast-grep.github.io/reference/sgconfig) and the [0.45.3 project schema](https://github.com/ast-grep/ast-grep/blob/0.45.3/schemas/project.json).

| Field                | Shape                     | Purpose                                                                                                                                       |
| -------------------- | ------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| `ruleDirs`           | list of strings           | Directories containing YAML rules.                                                                                                            |
| `testConfigs`        | list of objects           | Rule-test locations. Each object requires `testDir` and may set `snapshotDir`.                                                                |
| `utilDirs`           | list of strings           | Directories containing global utility rules.                                                                                                  |
| `languageGlobs`      | language to glob-list map | Overrides extension-based parser selection.                                                                                                   |
| `customLanguages`    | name to object map        | Registers a dynamic parser with `libraryPath` and `extensions`; optional fields are `outlineRules`, `expandoChar`, and `languageSymbol`.      |
| `languageInjections` | list of objects           | Experimental embedded-language support. Each object has `hostLanguage`, a matching `rule`, and static or candidate-list `injected` languages. |

This Rust project needs only explicit `ruleDirs`, `testConfigs`, and `utilDirs`. Do not configure custom languages, language overrides, or experimental injections without a concrete repository file that needs them.

ast-grep has no configuration-version field. The tagged project schema declares JSON Schema draft-07, while the tagged rule schemas declare draft 2020-12. Editor and local gate validation must use the raw schemas from tag `0.45.3`, not SchemaStore or the moving `main` branch. The web documentation already lags 0.45.3 in several fields described below.

## Complete lint-rule configuration surface

A YAML file may contain multiple documents separated by `---`. The web reference requires `id`, `language`, and `rule`. The 0.45.3 schema requires only `language` and `rule`, and runtime accepts an empty ID. Project policy resolves that mismatch by requiring an explicit unique `id`. The complete field set below comes from the [official YAML reference](https://ast-grep.github.io/reference/yaml) and the [0.45.3 Rust rule schema](https://github.com/ast-grep/ast-grep/blob/0.45.3/schemas/rust_rule.json).

| Group          | Fields                                  |
| -------------- | --------------------------------------- |
| Identity       | `id`, `language`                        |
| Finding        | `rule`, `constraints`, `utils`          |
| Patching       | `transform`, `fix`, `rewriters`         |
| Diagnostics    | `severity`, `message`, `note`, `labels` |
| File selection | `files`, `ignores`                      |
| External data  | `url`, `metadata`                       |

Important details:

- `constraints` keys name single meta-variables without `$`. They do not apply to multi-node `$$$` variables and run after the main rule.
- Local `utils` are reusable rule objects referenced with `matches`. Global utilities live under `utilDirs`.
- `labels` map captured variable names to `primary` or `secondary` highlighting and may add editor messages.
- `files` includes matching paths. `ignores` runs first and excludes matching paths. Both accept strings or objects with `glob` and optional `caseInsensitive`.
- Rule globs are relative to the project root and must not start with `./`.
- `url` links rule documentation. `metadata` is emitted only when JSON output requests metadata.
- `severity` accepts `hint`, `info`, `warning`, `error`, or `off`. Its default is `hint`, which this project must never rely on.

Committed rules must not depend on matching defaults either. Use object-form patterns with an explicit `strictness`, set `stopBy` on every relational matcher, give every rule an explicit `files` scope, and set `snapshotDir` in the test configuration. Omit a field only when the rule does not use that feature.

Lint rules must omit `transform`, `fix`, and `rewriters`. Those fields are useful for separately reviewed codemods, but a quality gate must report and fail without modifying files. The patching language supports `replace`, `substring`, `convert`, and `rewrite` transformations, plus string fixes and expanded-range fix objects. See the [transformation reference](https://ast-grep.github.io/reference/yaml/transformation) and [fix reference](https://ast-grep.github.io/reference/yaml/fix).

## Complete rule-object language

Every rule object needs at least one positive matcher. A rule object combines its fields as an unordered conjunction. Use an explicit `all` list when capture order matters. The [rule-object reference](https://ast-grep.github.io/reference/rule) defines the complete set.

### Atomic matchers

| Matcher    | Meaning                                                                                                                                 |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `pattern`  | Matches one parsed syntax node. It accepts a code string or an object with required `context` and optional `selector` and `strictness`. |
| `kind`     | Matches a Tree-sitter node kind. Version 0.39 and later also accepts the documented limited ESQuery form.                               |
| `regex`    | Matches the entire text of a node with Rust regex syntax. Rust regex has no look-around or backreferences.                              |
| `nthChild` | Matches a numbered or `An+B` sibling position. Object form adds `reverse` and an `ofRule` sibling filter.                               |
| `range`    | Restricts matching to required zero-based `start` and `end` positions. Each position requires `line` and may add `column`.              |

Pattern strictness values in 0.45.3 are `cst`, `smart`, `ast`, `relaxed`, `signature`, and `template`. `template` ignores node kinds and matches text, so do not use it in initial lint rules. The stable web rule reference still lists only the first five values, while the pinned schema and source include `template`. For lint rules, prefer `cst` or `smart`. A relaxed pattern is acceptable only when tests prove that omitted syntax cannot change the rule's meaning.

Pattern meta-variables are part of the matching language. `$NAME` captures one named node, `$$NAME` captures one unnamed node, and `$$$NAME` captures zero or more nodes. A name beginning with `_` does not capture, so `$_` is the ordinary anonymous wildcard. See the [official pattern syntax](https://ast-grep.github.io/guide/pattern-syntax).

The limited ESQuery form of `kind` supports direct child `>`, descendant space, adjacent sibling `+`, following sibling `~`, comma alternatives, compound selectors, and the `:has`, `:not`, `:is`, `:nth-child`, and `:nth-last-child` pseudo-classes. It rejects class selectors and other pseudo-classes. The [ESQuery reference](https://ast-grep.github.io/reference/rule/esquery) lists the remaining restrictions.

### Relational matchers

`inside`, `has`, `precedes`, and `follows` take another rule. All four accept `stopBy`, whose values are `neighbor`, `end`, or a rule object. The default is `neighbor`. Only `inside` and `has` accept `field` to select a named grammar field.

### Composite matchers

- `all` requires every sub-rule to match.
- `any` requires one sub-rule to match.
- `not` rejects a sub-rule match.
- `matches` invokes a local or global utility rule.

Utility dependency cycles are invalid. Recursive utilities are supported only when recursion progresses through a relational matcher such as `inside` or `has`. The official [utility-rule guide](https://ast-grep.github.io/guide/rule-config/utility-rule) explains this boundary.

Version 0.45.3 also has experimental parameterized global utilities. A global utility declares required `arguments`, and `matches` receives an object mapping every argument name to a rule object. Local utilities cannot take arguments. The stable rule reference still documents `matches` as a string, while the official [0.42 announcement](https://ast-grep.github.io/blog/new-ver-42#parameterized-utilities-experimental) documents the newer object form and its performance warning. Do not use parameterized utilities in the initial policy. This documentation mismatch is another reason to pin the tool and validate against tag-specific schemas.

A normal global utility document has `id`, `language`, `rule`, and optional `constraints` and `utils`. The experimental form also has `arguments`. It does not have lint diagnostics or file globs because another lint rule invokes it and owns those fields.

## Strict scan behavior

The full structural gate should be equivalent to:

```text
ast-grep scan \
  --config sgconfig.yml \
  --error \
  --min-severity=error \
  --max-results=1 \
  --inspect=summary \
  .
```

The eventual Make target may wrap this command, but the pinned executable and explicit configuration path remain part of the contract. The single bare `--error` sets the default severity for every discovered project rule and both built-in suppression rules. Do not combine it with repeated `--error=<rule-id>` occurrences. The CLI collects these occurrences into one value list, which can remove the empty occurrence that requested a global default. `--min-severity=error`, added in 0.45.3, rejects accidental reliance on lower-severity output by filtering it from the scan. `--max-results=1` stops after the first failure for fast feedback. Every rule file still declares `severity: error` so editor and targeted local scans agree with the full local gate. No committed rule may use `severity: off`. The behavior comes from the pinned [severity override source](https://github.com/ast-grep/ast-grep/blob/0.45.3/crates/cli/src/utils/rule_overwrite.rs) and [built-in rule setup](https://github.com/ast-grep/ast-grep/blob/0.45.3/crates/cli/src/scan.rs).

Research against the official 0.45.3 x86-64 Linux release confirmed the exact command fails each case separately. Repeating `--error` with named IDs reproduced the lost-default failure. Before accepting setup, add a CLI contract test with three separate fixtures. One violates a normal project rule, one has an unused rule-specific suppression, and one uses a bare suppression. The exact command above must return nonzero for all three under the pinned binary. This catches changes in a subtle CLI contract.

The official scan contract returns status `1` when at least one error-severity rule matches and `0` when none match. Warnings, information, and hints do not fail a scan. The project treats every nonzero result as a gate failure, including invalid configuration and tool failures. See the [scan reference](https://ast-grep.github.io/reference/cli/scan) and [severity guide](https://ast-grep.github.io/guide/project/severity).

The complete severity override set is `--error`, `--warning`, `--info`, `--hint`, and `--off`, each globally or for named rule IDs. `--min-severity` filters after overrides and does not promote a rule. Project gates use only the one bare `--error`; `--off` is forbidden.

Use `--filter=<anchored-rule-id-regex>` for a targeted sibling gate. A filtered gate is fast feedback, not the suppression audit, because ast-grep intentionally disables automatic unused-suppression reporting when rule filtering could make a valid suppression look unused. The full scan must remain its own sibling gate and stay under the project's 60-second limit.

Do not use `--update-all`, `--interactive`, fixes, or rewrites in a quality gate. Structured local reports may use SARIF. `--inspect=summary` records how many rules and files ast-grep scanned or skipped.

## Suppression policy

ast-grep recognizes `ast-grep-ignore` on the same line or the preceding line. A bare directive suppresses every rule. A colon followed by comma-separated rule IDs suppresses only those rules. A first-line directive followed by a blank second line suppresses matching diagnostics for the whole file. The [official severity guide](https://ast-grep.github.io/guide/project/severity) documents these forms.

Project policy is stricter:

- Bare `ast-grep-ignore` is forbidden by `no-suppress-all` at error severity.
- Unused directives fail the full scan through `unused-suppression` at error severity.
- File-level suppression is forbidden, including rule-specific file suppression.
- A line suppression must name exactly the intended rule, sit next to the finding, and have a nearby ordinary comment explaining why the code is safe.
- Adding or changing a suppression requires a valid rule-test case proving that the exception stays narrow.
- Suppression counts are reviewable data. An increase is never hidden inside generated output.

The first two controls are built in. File-level rejection and any required exception-comment shape need project rules or a small text gate because a suppression comment can affect ast-grep's own view of a finding.

## Rule test gate

Each test YAML document has an `id`, a `valid` list that must not match, and an `invalid` list that must match. Snapshots also pin diagnostic spans, messages, notes, and labels. This is the complete supported case shape. See the [rule-test guide](https://ast-grep.github.io/guide/test-rule) and [`ast-grep test` reference](https://ast-grep.github.io/reference/cli/test).

The committed gate runs `ast-grep test --config sgconfig.yml` with snapshots enabled. `--skip-snapshot-tests` is for local rule authoring only. `--update-all` and `--interactive` may update snapshots during deliberate maintenance, never in a check-only hook or quality gate. Review snapshot changes like source changes.

The complete test command also offers `--test-dir`, `--snapshot-dir`, `--filter`, `--include-off`, `--follow`, and `--color`. Targeted local gates may use `--filter`. The full local gate uses the configured directories, does not follow symlinks, and never uses `--include-off` because committed off rules are forbidden.

Every project rule must test:

- the smallest violating form
- each qualified, imported, aliased, nested, async, and macro spelling the rule claims to cover
- the allowed seam or directory
- close valid forms that differ by one important syntax feature
- comments and strings containing the forbidden spelling
- its exact diagnostic and highlighted node

If a spelling cannot be detected because ast-grep lacks name or type resolution, narrow the written claim. Do not pretend the rule is complete.

Validate `sgconfig.yml` and rule YAML against schemas from the exact pinned tag, not the moving `main` branch. The CLI must also load every rule during tests and a full scan. Schema validation alone cannot prove that a pattern matches the intended Rust grammar.

## Complete official Rust catalog as of the research date

The [official Rust catalog](https://ast-grep.github.io/catalog/rust/) contains six entries. It is an example catalog, not a recommended strict lint set.

| Catalog entry                                      | Form                 | Project decision                                                                                                          |
| -------------------------------------------------- | -------------------- | ------------------------------------------------------------------------------------------------------------------------- |
| Avoid duplicated exports                           | YAML structural rule | Rework and test before adoption. Syntax alone cannot prove the intended public API, but the check may fit the crate root. |
| Beware of char offset when iterating over a string | Search and rewrite   | Do not enable universally. `chars().enumerate()` is correct when the caller wants character positions.                    |
| Get number of digits in a `usize`                  | Search and rewrite   | Do not enable universally. This is a local performance rewrite, not a correctness rule.                                   |
| Unsafe function without unsafe block               | YAML error rule      | Do not adopt. An `unsafe fn` can express a caller contract even when its body contains no `unsafe` block.                 |
| Rust 2024 let-chain candidate                      | YAML hint rule       | Leave to rustfmt and Clippy policy. It is a style suggestion, not a project invariant.                                    |
| Rewrite `indoc!` macro                             | Search and rewrite   | Do not adopt. The project has no confirmed need for this codemod.                                                         |

No catalog entry enters the gate by copying its example. Any adopted idea becomes a project-owned rule with an explicit error severity, project documentation, and the test matrix above.

## Proposed project structural rules

These are proposals to implement after the test environment gate permits setup work. Names and path scopes should follow the crate layout once it exists.

### Immediate syntax rules

| Proposed rule                        | Intended contract                                                                                                                                                                               |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `no-broad-lint-suppression`          | Reject crate, module, and item attributes that allow broad rustc or Clippy groups. Narrow named exceptions remain reviewable. Cover `allow`, `cfg_attr(..., allow(...))`, and inner attributes. |
| `lint-suppression-requires-reason`   | Reject `allow` or `expect` attributes without a non-empty `reason`, including attributes nested in `cfg_attr`. This mirrors the Rust lint policy.                                               |
| `no-file-level-ast-grep-suppression` | Reject first-line, blank-line suppression form even when it names a rule.                                                                                                                       |
| `no-ignored-tests`                   | Reject `#[ignore]` and `#[cfg_attr(..., ignore)]` so a committed test cannot silently leave its normal gate.                                                                                    |
| `no-real-sleep-in-tests`             | Reject direct `std::thread::sleep`, Tokio sleep, and imported spellings in unit, integration, oracle, and simulator tests. Tests use fake time or bounded readiness signals.                    |

These rules have stable written syntax and do not need the future module layout. They still require fixtures before they become policy.

### Architecture and path-dependent rules

| Proposed rule                                   | Intended contract                                                                                                                                                                 |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `no-real-clock-in-deterministic-tests`          | Reject direct `Instant::now`, `SystemTime::now`, Tokio instant access, and imported spellings outside clock-adapter contract tests.                                               |
| `no-os-randomness-in-deterministic-tests`       | Reject direct OS and thread RNG construction outside randomness-adapter contract tests. Tests use explicit seeds or fixtures.                                                     |
| `external-io-only-in-adapters`                  | Reject imports and fully qualified calls for Bluetooth, D-Bus, NetworkManager, sockets, filesystem, process, environment, clock, and random APIs in protocol and session modules. |
| `test-doubles-only-in-test-support`             | Reject mock-framework attributes and imports, plus project `Fake`, `Stub`, and `Mock` declarations, outside test-only paths or modules.                                           |
| `test-support-names-match-role`                 | Require test-support declarations to use the established `Builder`, `Factory`, `Fixture`, `Fake`, `Stub`, or `Mock` suffix for their directory and role.                          |
| `assertions-only-in-tests-or-assertion-helpers` | Reject assertion macros in ordinary setup helpers, builders, factories, fixtures, fakes, stubs, and mocks. Named assertion helpers remain allowed.                                |
| `mocks-only-at-external-seams`                  | Restrict mock declarations to test-support paths for the confirmed local-control, transfer, connection, and oracle interfaces. Project modules must not mock each other.          |
| `no-detached-task-spawn`                        | Reject bare spawn expressions and spawning outside the lifecycle owner. Every background task must have an owned cancellation and join path.                                      |
| `typed-control-protocol`                        | Reject `serde_json::Value` and untyped JSON maps in the versioned Unix-socket protocol module. Boundary messages use declared request, response, and event types.                 |
| `no-sensitive-debug-derive`                     | Reject `Debug` derivation on explicitly named key, token, credential, and decrypted-payload types unless the type has a reviewed redacted implementation.                         |

These rules complement Clippy because their meaning comes from project paths, confirmed seams, or local type names. Start each rule only after those names and directories exist.

### Rules to consider after code exists

- Keep BlueZ, NetworkManager, D-Bus, socket, storage, and oracle APIs out of the domain layer.
- Keep direct file creation and rename operations inside the transfer-storage adapter.
- Keep task spawning and signal handling inside the process supervisor.
- Reject production imports from test-support modules.
- Reject assertion, debug-print, placeholder, and panic macros in the long-running daemon outside a documented fatal-startup boundary.
- Reject logging fields whose project names denote filenames, peer names, addresses, keys, authentication tokens, or payload bytes.
- Require security-sensitive types to use redacted wrappers instead of ordinary `String` or `Vec<u8>` fields where a stable naming rule can identify them.
- Reject direct environment-variable reads outside the configuration adapter.
- Restrict raw `Command` execution to the oracle and explicitly approved system adapters.

Each item needs real project syntax before it can become a reliable rule. Imported aliases and wrapper functions can evade simple call patterns, so architecture rules should check both forbidden imports and common fully qualified calls.

## What ast-grep cannot prove

ast-grep parses concrete syntax. Its own FAQ says it has no scope, type, control-flow, data-flow, taint, or constant-propagation analysis. It cannot establish which trait method a call resolves to, whether a `Result` was discarded, whether a task always joins, or whether sensitive data reaches a logger. Those claims belong to rustc, Clippy, purpose-built analysis, or tests. See the [official limitations](https://ast-grep.github.io/advanced/faq#does-ast-grep-support-some-advanced-static-analysis).

Rust macros are another hard boundary. ast-grep 0.45.3 locks `tree-sitter-rust` 0.24.2, and that grammar represents a macro invocation and its token tree as syntax nodes. ast-grep can match the written invocation, but it does not run Rust macro expansion. This is an inference from the pinned [dependency lock](https://github.com/ast-grep/ast-grep/blob/0.45.3/Cargo.lock) and [Rust grammar](https://github.com/tree-sitter/tree-sitter-rust/blob/v0.24.2/grammar.js). Generated code, derive output, name resolution across imports, and `cfg` evaluation remain invisible.

Parser recovery can also produce different trees for invalid or incomplete snippets. The CLI and playground may use different parser versions. Test rules against complete compilable Rust snippets with the pinned CLI, using pattern `context` and `selector` when a fragment is ambiguous. The [official FAQ](https://ast-grep.github.io/advanced/faq) documents these parser limits.

Do not use ast-grep to enforce the 500 or 800 line limits, gate duration, dependency licenses, vulnerability status, MSRV, public API compatibility, protocol state transitions, cryptographic behavior, cleanup, or connection interoperability. Those already have better gates.

## Adoption checklist for later

No setup is authorized by this research document. When setup begins, the change should include all of the following together:

1. An exact 0.45.3 tool pin and per-platform checksum.
2. A hand-written root configuration with only required directories.
3. Pinned 0.45.3 project and Rust rule schemas.
4. A rule-test gate with committed snapshots.
5. A full error-only scan that enables both built-in suppression rules.
6. Targeted rule-family gates that each finish within 60 seconds.
7. Two-line `AGENTS.md` instructions for every new or split gate.
8. A Make target that exposes the full gate and its targeted siblings.
