#!/usr/bin/env bash

set -euo pipefail

readonly ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly TOOLS_ROOT="${ROOT}/.cache/tools"
readonly CARGO_MACHETE_VERSION="0.9.2"
readonly CARGO_MACHETE_ROOT="${TOOLS_ROOT}/cargo-machete-\
${CARGO_MACHETE_VERSION}"
readonly CARGO_MACHETE="${CARGO_MACHETE_ROOT}/bin/cargo-machete"
readonly VULTURE_VERSION="2.16"
readonly VULTURE_ROOT="${TOOLS_ROOT}/vulture-${VULTURE_VERSION}"
readonly VULTURE="${VULTURE_ROOT}/bin/vulture"
readonly VULTURE_WHEEL="vulture-${VULTURE_VERSION}-py3-none-any.whl"
readonly VULTURE_URL="https://files.pythonhosted.org/packages/f5/be/\
f935130312330614811dae2ea9df3f395f6d63889eb6c2e68c14507152ee/${VULTURE_WHEEL}"
readonly VULTURE_SHA256="6e0f1c312cef1c87856957e5c2ca9608\
834a7c794c2180477f30bf0e4cc58eee"
readonly CLANG_TIDY_VERSION="22.1.8"
readonly CPPCHECK_VERSION="2.21.1"

mkdir -p -- "${TOOLS_ROOT}"

if [[ ! -x "${CARGO_MACHETE}" ]] ||
  [[ "$("${CARGO_MACHETE}" --version 2>/dev/null)" != \
    "${CARGO_MACHETE_VERSION}" ]]; then
  rm -rf -- "${CARGO_MACHETE_ROOT}"
  cargo install cargo-machete \
    --version "${CARGO_MACHETE_VERSION}" \
    --locked \
    --root "${CARGO_MACHETE_ROOT}"
fi

if [[ ! -x "${VULTURE}" ]] ||
  [[ "$("${VULTURE}" --version 2>/dev/null)" != \
    "vulture ${VULTURE_VERSION}" ]]; then
  rm -rf -- "${VULTURE_ROOT}"
  python -m venv "${VULTURE_ROOT}"
  readonly temporary_directory="$(mktemp -d)"
  trap 'rm -rf -- "${temporary_directory}"' EXIT
  curl --fail --location --proto '=https' --tlsv1.2 \
    --output "${temporary_directory}/${VULTURE_WHEEL}" \
    "${VULTURE_URL}"
  printf '%s  %s\n' \
    "${VULTURE_SHA256}" \
    "${temporary_directory}/${VULTURE_WHEEL}" |
    sha256sum --check --strict
  "${VULTURE_ROOT}/bin/python" -m pip install \
    --disable-pip-version-check \
    --no-deps \
    "${temporary_directory}/${VULTURE_WHEEL}"
fi

clang_tidy_output="$(clang-tidy --version)"
if [[ "${clang_tidy_output}" != *"LLVM version ${CLANG_TIDY_VERSION}"* ]]; then
  printf 'expected clang-tidy %s, received:\n%s\n' \
    "${CLANG_TIDY_VERSION}" "${clang_tidy_output}" >&2
  exit 1
fi
cppcheck_output="$(cppcheck --version)"
if [[ "${cppcheck_output}" != "Cppcheck ${CPPCHECK_VERSION}" ]]; then
  printf 'expected Cppcheck %s, received %s\n' \
    "${CPPCHECK_VERSION}" "${cppcheck_output}" >&2
  exit 1
fi
