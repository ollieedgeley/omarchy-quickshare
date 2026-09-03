#!/bin/bash

set -euo pipefail

readonly RUFF_VERSION="0.16.5"
readonly RUFF_SHA256="65b8bae7e43f12a91b71036a52176012"\
"b3aefb725d5ae263e2771474110a0983"
readonly RUFF_ARCHIVE="ruff-x86_64-unknown-linux-gnu.tar.gz"
readonly RUFF_URL="https://github.com/astral-sh/ruff/releases/download/"\
"${RUFF_VERSION}/${RUFF_ARCHIVE}"
readonly REPOSITORY_ROOT="$(git rev-parse --show-toplevel)"
readonly TOOL_ROOT="${REPOSITORY_ROOT}/.cache/tools"
readonly DESTINATION="${TOOL_ROOT}/ruff-${RUFF_VERSION}"
readonly RUFF_BINARY="${DESTINATION}/ruff"

if [[ -x "${RUFF_BINARY}" ]]; then
  "${RUFF_BINARY}" --version | grep -qx "ruff ${RUFF_VERSION}"
  exit 0
fi

mkdir -p "${TOOL_ROOT}"
temporary_directory="$(mktemp -d "${TOOL_ROOT}/ruff.XXXXXX")"
trap 'rm -rf "${temporary_directory}"' EXIT
archive="${temporary_directory}/${RUFF_ARCHIVE}"
curl --fail --location --silent --show-error \
  --output "${archive}" "${RUFF_URL}"
echo "${RUFF_SHA256}  ${archive}" | sha256sum --check --status
mkdir "${temporary_directory}/extract"
tar -xzf "${archive}" --strip-components=1 \
  -C "${temporary_directory}/extract"
rm -rf "${DESTINATION}"
mv "${temporary_directory}/extract" "${DESTINATION}"
"${RUFF_BINARY}" --version | grep -qx "ruff ${RUFF_VERSION}"
