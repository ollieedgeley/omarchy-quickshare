#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
generator="$root/tools/codegen/quickshare-wire/Cargo.toml"

cargo run --quiet --manifest-path "$generator" --locked
