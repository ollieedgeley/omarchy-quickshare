# Omarchy Quick Share

This repository is preparing a lightweight, bidirectional Quick Share endpoint for Omarchy. Application code has not started: the compatibility research, stable workspace boundaries, programmatic connection-test seams, and local quality system are being established first.

## Development setup

Install the pinned development dependencies and activate the local hooks:

```sh
make setup
```

Use `make help` for targeted feedback. `make verify` is the complete local quality suite, while `make build` performs the locked Cargo build. Pre-commit checks the staged snapshot and affected Cargo packages; pre-push verifies and then builds the exact commit being pushed.

The Node packages, CodeGraph database, simulators, protocol oracles, and other test dependencies are development-only. The future native binary will retain a locked source-build path that excludes this tooling, and the Omarchy plugin will remain independently installable.

Start with [CONTEXT.md](CONTEXT.md), then follow the task-specific links in [AGENTS.md](AGENTS.md).
