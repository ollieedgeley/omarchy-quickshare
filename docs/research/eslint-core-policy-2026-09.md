# ESLint core-rule policy

Research date: 2026-09-02

## Decision

Pin ESLint, `@eslint/js`, and the Node globals catalogue in the development manifest. Apply `js/all` to every project-authored JavaScript module and reject warnings, inline configuration, unused disable directives, and unused inline configuration. An executable contract compares the resolved configuration with ESLint's installed non-deprecated core-rule catalogue so a rule cannot be silently omitted or downgraded.

Keep Prettier as the single JavaScript formatter. ESLint owns semantic and structural policy; it does not duplicate deprecated formatting rules. Full checks are divided into application and tooling aggregates. Staged checks select by file type, while `make verify` remains the authoritative complete suite.

The former ESLint core `max-len` rule is deprecated. Use the pinned ESLint Stylistic replacement for the repository's line-length policy. [ESLint Stylistic `max-len`](https://eslint.style/rules/max-len), [ESLint Stylistic installation](https://eslint.style/rules)

## Evidence

ESLint 10 only supports flat configuration. The official migration guide identifies `@eslint/js` and its `all` configuration as the supported replacement for the old `eslint:all` string. The package describes `all` as enabling every ESLint rule. [ESLint 10 migration](https://eslint.org/docs/latest/use/migrate-to-10.0.0), [`@eslint/js` usage](https://github.com/eslint/eslint/tree/v10.9.1/packages/js)

ESLint rule severities are `off`, `warn`, and `error`; only `error` necessarily produces a failing exit status. ESLint also supports rejecting inline configuration and reporting unused disable or inline-config comments. The project sets those policies in the checked-in flat config and still invokes the CLI with zero tolerated warnings as a defence in depth. [Rule severity and inline configuration](https://eslint.org/docs/latest/use/configure/rules), [CLI warning and directive controls](https://eslint.org/docs/latest/use/command-line-interface)

The `all` preset contains current, non-deprecated core rules. Deprecated rules are unmaintained and may be removed after an equivalent replacement exists, so enabling them would make a frozen duplicate policy rather than broaden current coverage. The contract therefore measures the installed non-deprecated catalogue. [ESLint rule deprecation policy](https://eslint.org/docs/latest/use/rule-deprecation), [complete rule reference](https://eslint.org/docs/latest/rules/)

ESLint 10.9.1 was released on 2026-08-24 and corrects false positives introduced in 10.9.0. ESLint 9 reached end of life on 2026-08-06. [ESLint 10.9.1 release](https://eslint.org/blog/2026/08/eslint-v10.9.1-released/)

## Configuration authority

Every current core rule stays enabled at error severity. Executable configuration and contract tests are authoritative; documentation must not duplicate option values or thresholds. Inline suppressions are unavailable. A genuine conflict must be resolved once in the root configuration and protected by the policy contract.

## Gate separation

`make verify-app` covers only Rust application formatting, compiler checks, diagnostics, ast-grep, and Rust tests. `make verify-tooling` covers JavaScript and repository tooling formatting, linting, static contracts, and fast tooling tests. Neither aggregate starts a simulator or virtual device. Environment-specific targets exercise the changed environment, and `make verify` combines both aggregates with every programmatic connection check before push.

The pre-commit hook follows the same boundary. JavaScript changes run Prettier, ESLint, and tooling contract tests against the staged snapshot. Rust-only changes do not run JavaScript or environment tests unless the affected-test selector finds a real dependency. The complete pre-push suite remains intentionally broader.
