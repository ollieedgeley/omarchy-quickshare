# Omarchy Quick Share

This repository contains the early application and verification system for a
lightweight, bidirectional Quick Share endpoint for Omarchy. The CLI can submit
typed text, URL, and file requests to its local control endpoint. The current
development build does not discover peers or transfer attachments yet.

## Development setup

Install the pinned development dependencies and activate the local hooks:

```sh
make setup
```

Use `make help` for targeted feedback. `make verify` is the complete local quality suite, while `make build` performs the locked Cargo build. Pre-commit checks the staged snapshot and affected Cargo packages; pre-push verifies and then builds the exact commit being pushed.

The Node packages, CodeGraph database, simulators, protocol oracles, and other
test dependencies are development-only. The native binary retains a locked
source-build path that excludes this tooling, and the Omarchy plugin remains
independently installable.

The plugin source lives in `packaging/omarchy-plugin`. It is a development
preview that displays the installed binary's compatibility without claiming
that device discovery or transfers work. After committing, `make plugin-export`
creates its validated local Git repository under `dist/omarchy-plugin`.

Start with [CONTEXT.md](CONTEXT.md), then follow the task-specific links in [AGENTS.md](AGENTS.md).
