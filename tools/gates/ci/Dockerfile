ARG BASE_ID=818793c894d94534c22f2149154a39ebaee57e4e67321023b0866a1d5722036c
FROM archlinux:base@sha256:${BASE_ID}

SHELL ["/bin/bash", "-Eeuo", "pipefail", "-c"]

# Keep signature verification; resolve all packages from one archive snapshot.
RUN printf '%s\n' \
        'Server = https://archive.archlinux.org/repos/'\
'2026/08/25/$repo/os/$arch' \
        > /etc/pacman.d/mirrorlist \
    && pacman -Syuu --noconfirm --needed \
        base-devel git nodejs rust quickshell ttf-dejavu \
    && pacman -Scc --noconfirm

RUN set -x \
    && [[ "$(node --version)" == 'v26.7.0' ]] \
    && [[ "$(rustc --version)" == 'rustc 1.98.0 '* ]] \
    && [[ "$(cargo --version)" == 'cargo 1.98.0 '* ]] \
    && quickshell_version="$(quickshell --version)" \
    && [[ "$quickshell_version" == 'Quickshell 0.3.1' \
        || "$quickshell_version" == 'Quickshell 0.3.1 '* ]]

COPY --chmod=0755 entrypoint.sh /usr/local/bin/ci-pilot-entrypoint

ENV HOME=/tmp/home \
    XDG_RUNTIME_DIR=/tmp/runtime \
    CARGO_HOME=/cargo \
    CARGO_TARGET_DIR=/target \
    QT_QPA_PLATFORM=offscreen \
    QT_QUICK_BACKEND=software

# The runner overrides this with its own UID:GID and supplies writable caches.
USER 65532:65532
WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/ci-pilot-entrypoint"]
