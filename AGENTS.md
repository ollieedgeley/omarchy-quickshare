# Repository instructions

## Scope and work loop

- Work within the agreed task and the user's current scope.
- Preserve user changes.
- Confirm planned paths and commands exist before using them.
- Make the smallest complete change.
- Finish with the requested result and relevant checks passing.

## Read before starting

- Terminology: use the vocabulary in [CONTEXT.md](CONTEXT.md).
- Making project structure decisions: use [structure guidance](docs/project-structure.md).
- How to write Documentation: Follow [documentation policy](docs/documentation.md).
- Any plan, research, spec or ticket files should go in `.scratch` and not the repo `docs/`.

## CodeGraph
- Use codegraph. If its not initialised, run `codegraph init`.
- Use this tool to explore the codebase and get focused file sets and more. `codegraph --help` for more.

## Engineering discipline

### File Sizes

- Code files should be no more than 500 Lines long.
- Test Files should be no more than 800 Lines long.

Going over these limit is an indication to make a decision. Use [structure guidance](docs/project-structure.md) to make that decision.

### TDD
- TDD should be used when writing application code
- Don't write tests for repo configuration or tooling

## Git workflow

- Check the Makefile for available checks
- Lean heavily on tracked Husky hooks. There's no point running checks manually if they are going to be run anyway by hooks.
- When running checks manually, choose the narrowest existing command that observes the relevant contract
- Utilise CodeGraph for checks.
- Treat CodeGraph output as candidate data, not the final test scope.
- Fix failures before broadening the check.
- Leave aggregate formatting, linting, analysis, verification, and build gates to Git hooks.
- Never manually invoke `make pre-commit`, `make pre-push`, `make verify`, or `make build`.
- Push each authorized slice to the approved remote.
- Never use `--no-verify`, disabled tests, red or `WIP` commits, or force pushes.

## Waiting for commands

- Long running commands should be run as OMP-managed background jobs.
- Do independent work while they run; otherwise use `hub wait` for the job with `timeoutMs: 0`.
- Rely on completion notifications. Check progress only for a specific problem or a user request.
- Inspect the final result before reporting success or starting dependent work.
