#!/usr/bin/env bash
set -Eeuo pipefail

mkdir -p -- "${HOME:?}" "${XDG_RUNTIME_DIR:?}"
chmod 700 -- "$HOME" "$XDG_RUNTIME_DIR"
exec "$@"
