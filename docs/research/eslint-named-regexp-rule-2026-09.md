# ESLint rules for named regular expressions

Research cutoff: 2026-09-02

## Decision

No maintained third-party ESLint rule enforces this project's policy:

> Every regular-expression literal and every `RegExp` construction must first
> be assigned directly to a named variable.

Keep the current core `no-restricted-syntax` selectors. They enforce the
placement rule more closely than any published plugin rule found in the
current regex-specific and security-plugin catalogs. Adding another package
would either check a different property or replace an AST check with a brittle
source-text match.

The current selectors are not a perfect semantic rule. If the project later
needs binding-aware treatment of the global `RegExp` constructor, replace them
with a small project-owned ESLint rule and contract tests. Do not substitute
one of the near matches below.

## Current rule and exact coverage

ESLint's core `no-restricted-syntax` rule accepts esquery selectors and a
message for each forbidden AST shape. That is the intended use of the rule,
not an unsupported workaround.
[ESLint rule documentation](https://eslint.org/docs/latest/rules/no-restricted-syntax)

The repository currently configures these selectors:

```javascript
{
  selector: "Literal[regex]:not(VariableDeclarator > Literal[regex])",
  message: "Assign this regular expression to a named variable.",
},
{
  selector:
    "CallExpression[callee.name='RegExp']:not(" +
    "VariableDeclarator > CallExpression[callee.name='RegExp'])",
  message: "Assign this RegExp construction to a named variable.",
},
{
  selector:
    "NewExpression[callee.name='RegExp']:not(" +
    "VariableDeclarator > NewExpression[callee.name='RegExp'])",
  message: "Assign this RegExp construction to a named variable.",
},
```

They accept the three direct forms:

```javascript
const LITERAL_PATTERN = /value/u;
const CALL_PATTERN = RegExp(source, "u");
const NEW_PATTERN = new RegExp(source, "u");
```

They reject inline literals and constructions in calls, returns, conditions,
properties, arrays, and later assignments. That matches the stated policy that
the regex expression itself must first appear as a variable initializer.

Their known gaps are:

- A `VariableDeclarator` with a destructuring pattern is accepted even though
  its initializer does not have one identifier name.
- `globalThis.RegExp(...)`, `new globalThis.RegExp(...)`, and aliases of the
  built-in constructor are not matched.
- A locally shadowed function named `RegExp` is matched even though it is not
  the built-in constructor.
- A regex-producing tag or factory is outside the policy because it is neither
  a regex literal nor a `RegExp` construction.

The destructuring gap can be closed syntactically by requiring
`VariableDeclarator[id.type="Identifier"]`. Qualified constructors, aliases,
and shadowing require scope and binding analysis. A source-text plugin cannot
resolve them.

## Candidate comparison

| Package and rule                                                                   | Exact configuration                               | What it checks                                                     | Why it cannot replace the selectors                                                                                                                 |
| ---------------------------------------------------------------------------------- | ------------------------------------------------- | ------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `eslint-plugin-regexp` 3.2.0                                                       | No matching rule exists                           | Regex syntax, flags, complexity, captures, replacements, and use   | Its complete exported catalog has no expression-placement or variable-assignment rule.                                                              |
| `eslint-plugin-sonarjs` 4.2.0, `sonarjs/stateful-regex`                            | `"sonarjs/stateful-regex": "error"`               | Unsafe uses of stateful global or sticky regexes                   | It requests extraction only in one unsafe loop case; ordinary inline regex expressions remain valid.                                                |
| `@rushstack/eslint-plugin-security` 0.14.2, `@rushstack/security/no-unsafe-regexp` | `"@rushstack/security/no-unsafe-regexp": "error"` | A `new RegExp(...)` pattern must be a string literal               | It ignores regex literals, `RegExp(...)` calls, and placement. Its own valid example uses an inline construction. Its peer range stops at ESLint 9. |
| `eslint-plugin-security` 4.0.1, `security/detect-non-literal-regexp`               | `"security/detect-non-literal-regexp": "error"`   | Reports `new RegExp(...)` when its pattern is not statically known | It ignores call-form and literal regexes, accepts inline static constructions, and does not inspect placement.                                      |
| `eslint-plugin-regex` 1.11.0, `regex/invalid` and `regex/required`                 | `"regex/invalid": ["error", ["pattern"]]`         | Matches forbidden or required text in the whole source file        | It has no JavaScript AST or binding model. Formatting, comments, strings, and multiline expressions make an assignment policy unreliable.           |

### `eslint-plugin-regexp`

This is the strongest maintained regex plugin and is already a dependency of
the repository. Version 3.2.0 was published on 2026-08-13. It requires ESLint
9.38 or later, supports the repository's ESLint 10.9.1, and tests against
ESLint 10 in its own development dependencies.
[3.2.0 package manifest](https://github.com/ota-meshi/eslint-plugin-regexp/blob/v3.2.0/package.json)

The plugin exports 82 rules from one explicit list. The list includes rules
such as `regexp/no-invalid-regexp`, `regexp/no-super-linear-backtracking`,
`regexp/prefer-regexp-exec`, and `regexp/require-unicode-regexp`. There is no
rule about assigning a regex expression to a variable.
[complete 3.2.0 rule list](https://github.com/ota-meshi/eslint-plugin-regexp/blob/v3.2.0/lib/all-rules.ts),
[published rule catalog](https://github.com/ota-meshi/eslint-plugin-regexp/blob/v3.2.0/README.md#rules)

Its regex visitor can understand literals and many global `RegExp` calls. That
does not make every possible policy available as a rule. The plugin exports
only the modules in `all-rules.ts`, so there is no hidden assignment rule to
enable through `flat/all`.
[plugin export](https://github.com/ota-meshi/eslint-plugin-regexp/blob/v3.2.0/lib/index.ts),
[`flat/all` construction](https://github.com/ota-meshi/eslint-plugin-regexp/blob/v3.2.0/lib/configs/rules/all.ts)

No `regexp/*` setting should be proposed as the replacement. The installed
recommended rules remain useful alongside the placement selectors because
they inspect the pattern after the placement check has passed.

### SonarJS stateful-regex rule

`sonarjs/stateful-regex` is the closest maintained rule by message: in one
case it says to extract a regular expression. Its implementation visits regex
literals plus call-form and constructor-form `RegExp`, but reports extraction
only when a global regex is the receiver of `exec()` inside a `while` or
`do...while` condition. Its other checks concern conflicting flags and reused
stateful expressions, not placement. Ordinary inline regex expressions are
accepted.
[4.2.0 rule source](https://github.com/SonarSource/SonarJS/blob/8cb7855fdbf857c00674edbf2e2b421bda4dbd49/packages/analysis/src/jsts/rules/S6351/rule.ts)

Version 4.2.0 was published on 2026-07-14 and explicitly declares support for
ESLint 8, 9, and 10. It is current and compatible, but enabling it would add a
separate state-safety check rather than enforce the named-variable policy.
[4.2.0 package metadata](https://www.npmjs.com/package/eslint-plugin-sonarjs/v/4.2.0)

### Rush Stack security rule

Rush Stack's rule name sounds broader than its implementation. Version 0.14.2
registers only a `NewExpression` visitor. It matches the identifier `RegExp`
and reports when the first constructor argument is not a literal. It neither
visits regex literals nor checks the parent node.
[0.14.2 implementation](https://github.com/microsoft/rushstack/blob/3b13f32b28e2ee19d856d8e60dd00b7675855d4a/eslint/eslint-plugin-security/src/no-unsafe-regexp.ts)

The package documentation explicitly accepts this inline form:

```javascript
return new RegExp("[0-9]+").test(value);
```

That is exactly what this project's rule rejects.
[0.14.2 examples](https://github.com/microsoft/rushstack/blob/3b13f32b28e2ee19d856d8e60dd00b7675855d4a/eslint/eslint-plugin-security/README.md)

The package's declared ESLint peer range is 6 through 9. It does not declare
ESLint 10 compatibility, so it is unsuitable here even before the semantic
mismatch.
[0.14.2 package manifest](https://github.com/microsoft/rushstack/blob/3b13f32b28e2ee19d856d8e60dd00b7675855d4a/eslint/eslint-plugin-security/package.json)

### ESLint Community security rule

`security/detect-non-literal-regexp` visits only `NewExpression` nodes. It
checks the callee name and asks whether the first argument is statically known.
It does not visit call-form `RegExp(...)`, regex literals, or the expression's
parent, so direct assignment and inline use are equivalent to this rule.
[4.0.1 source](https://github.com/eslint-community/eslint-plugin-security/blob/eslint-plugin-security-v4.0.1/rules/detect-non-literal-regexp.js),
[rule documentation](https://github.com/eslint-community/eslint-plugin-security/blob/eslint-plugin-security-v4.0.1/docs/rules/detect-non-literal-regexp.md)

Version 4.0.1 was published on 2026-06-12. It is maintained, but its package
does not declare an ESLint peer range. More importantly, its valid case is
`new RegExp("ab+c", "i")` regardless of where that expression appears.
[4.0.1 tests](https://github.com/eslint-community/eslint-plugin-security/blob/eslint-plugin-security-v4.0.1/test/rules/detect-non-literal-regexp.js),
[4.0.1 package manifest](https://github.com/eslint-community/eslint-plugin-security/blob/eslint-plugin-security-v4.0.1/package.json)

### Source-text regex plugin

`eslint-plugin-regex` 1.11.0 was published on 2026-08-03 and declares
`eslint >=4`, which includes ESLint 10. Its rules apply user-provided regular
expressions to raw source text. `regex/invalid` rejects matching text and
`regex/required` requires matching text.
[1.11.0 package documentation](https://www.npmjs.com/package/eslint-plugin-regex/v/1.11.0)

A text pattern cannot reliably distinguish a regex literal from division,
ignore comments and string contents while retaining code, resolve the global
constructor, or prove the AST parent is a variable declarator. This package is
maintained, but it is the wrong parser for the policy.

## If the current gaps become important

A project-owned rule can improve coverage without another runtime dependency.
It should visit regex literals, `CallExpression`, and `NewExpression`; resolve
the `RegExp` binding through ESLint's scope manager; accept only a direct
`VariableDeclarator` whose `id` is an `Identifier`; and report every other
placement. Its contract tests should include global, qualified, aliased,
shadowed, destructured, exported, inline, and optional-chain cases.

That would be a behavior change, not a dependency swap. Until those extra
semantics are required, the three declarative selectors are smaller, easier to
audit, and more compatible than the available plugin rules.
