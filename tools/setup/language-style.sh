#!/usr/bin/env bash

set -euo pipefail

readonly ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
readonly TOOLS_ROOT="${ROOT}/.cache/tools"
readonly JRE_ROOT="${TOOLS_ROOT}/temurin-jre-21.0.12.1+1"
readonly JRE_URL="https://github.com/adoptium/temurin21-binaries/releases/\
download/jdk-21.0.12.1%2B1/\
OpenJDK21U-jre_x64_linux_hotspot_21.0.12.1_1.tar.gz"
readonly JRE_SHA256="2413149700df0f7d440500a84a8f764c\
535f21e5a5e87d38328b64eec2c5b500"
readonly CHECKSTYLE_ROOT="${TOOLS_ROOT}/checkstyle-14.1.0"
readonly CHECKSTYLE_URL="https://github.com/checkstyle/checkstyle/releases/\
download/checkstyle-14.1.0/checkstyle-14.1.0-all.jar"
readonly CHECKSTYLE_SHA256="51e2bc7fed1bb56808aa39045f655a31\
6194997acd24bac5195253dcf342b380"
readonly LIZARD_ROOT="${TOOLS_ROOT}/lizard-1.24.0"

if [[ "$(uname -s)" != Linux || "$(uname -m)" != x86_64 ]]; then
  printf 'language style tools require Linux x86_64\n' >&2
  exit 1
fi

mkdir -p -- "${TOOLS_ROOT}"
readonly temporary_directory="$(mktemp -d \
  "${TOOLS_ROOT}/language-style.XXXXXX")"
trap 'rm -rf -- "${temporary_directory}"' EXIT

if [[ ! -x "${JRE_ROOT}/bin/java" ]]; then
  curl --fail --location --proto '=https' --tlsv1.2 \
    --output "${temporary_directory}/jre.tar.gz" "${JRE_URL}"
  printf '%s  %s\n' "${JRE_SHA256}" "${temporary_directory}/jre.tar.gz" |
    sha256sum --check --strict
  mkdir -- "${temporary_directory}/jre"
  tar -xzf "${temporary_directory}/jre.tar.gz" --strip-components=1 \
    -C "${temporary_directory}/jre"
  test -x "${temporary_directory}/jre/bin/java"
  mv -T -- "${temporary_directory}/jre" "${JRE_ROOT}"
fi

if [[ ! -f "${CHECKSTYLE_ROOT}/checkstyle.jar" ]]; then
  curl --fail --location --proto '=https' --tlsv1.2 \
    --output "${temporary_directory}/checkstyle.jar" "${CHECKSTYLE_URL}"
  printf '%s  %s\n' \
    "${CHECKSTYLE_SHA256}" "${temporary_directory}/checkstyle.jar" |
    sha256sum --check --strict
  mkdir -p -- "${CHECKSTYLE_ROOT}"
  mv -T -- "${temporary_directory}/checkstyle.jar" \
    "${CHECKSTYLE_ROOT}/checkstyle.jar"
fi
printf '%s  %s\n' \
  "${CHECKSTYLE_SHA256}" "${CHECKSTYLE_ROOT}/checkstyle.jar" |
  sha256sum --check --strict

python3 -m venv "${LIZARD_ROOT}"
"${LIZARD_ROOT}/bin/python" -m pip install \
  --disable-pip-version-check \
  --index-url https://pypi.org/simple \
  --only-binary=:all: \
  --require-hashes \
  --requirement "${ROOT}/tools/setup/language-requirements.txt"
