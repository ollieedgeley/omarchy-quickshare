# The provisioner supplies a sealed context with nearby/, overrides/, cache/,
# and assets/. Its values are pinned there, rather than guessed here.
ARG UBUNTU_IMAGE=\
ubuntu@sha256:33ceb71981b602c1a7443a53469e4dba065f7503eab3078a2d7a57a2ab987517
ARG CA_CERTIFICATES_PACKAGE=ca-certificates=20240203

FROM ${UBUNTU_IMAGE} AS builder

ARG CA_CERTIFICATES_PACKAGE
ARG UBUNTU_APT_SNAPSHOT_URL
ARG BUILDER_BASE_PACKAGES
ARG BUILDER_LLVM_PACKAGES
ARG LLVM_SIGNING_CERT_SHA256
ARG LLVM_APT_SOURCE
ARG BAZEL_SHA256=\
b4746d3ee7e698b08ebc6a91bd6b4062a25d1b4214a5964f866986f59fc159fb
ARG CLANG_VERSION=21

COPY cache/llvm-apt-signing-key.asc /tmp/llvm.asc
COPY cache/bazel-9.0.1-linux-x86_64 /tmp/bazel

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes \
      --no-install-recommends ${CA_CERTIFICATES_PACKAGE} \
    && rm -rf /var/lib/apt/lists/*

RUN test -n "${UBUNTU_APT_SNAPSHOT_URL}" \
    && test -n "${BUILDER_BASE_PACKAGES}" \
    && rm -f /etc/apt/sources.list.d/ubuntu.sources \
    && printf '%s\n' \
      "deb ${UBUNTU_APT_SNAPSHOT_URL} noble main universe" \
      "deb ${UBUNTU_APT_SNAPSHOT_URL} noble-updates main universe" \
      "deb ${UBUNTU_APT_SNAPSHOT_URL} noble-security main universe" \
      > /etc/apt/sources.list \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes \
      --no-install-recommends ${BUILDER_BASE_PACKAGES} \
    && test -n "${LLVM_SIGNING_CERT_SHA256}" \
    && test -n "${LLVM_APT_SOURCE}" \
    && echo "${LLVM_SIGNING_CERT_SHA256}  /tmp/llvm.asc" | sha256sum --check \
    && gpg --batch --dearmor --output /usr/share/keyrings/llvm.gpg \
      /tmp/llvm.asc \
    && printf '%s\n' "${LLVM_APT_SOURCE}" \
      > /etc/apt/sources.list.d/llvm.list \
    && apt-get update \
    && test -n "${BUILDER_LLVM_PACKAGES}" \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes \
      --no-install-recommends ${BUILDER_LLVM_PACKAGES} \
    && test -x "/usr/bin/clang-${CLANG_VERSION}" \
    && rm -rf /var/lib/apt/lists/* /tmp/llvm.asc

RUN echo "${BAZEL_SHA256}  /tmp/bazel" | sha256sum --check \
    && install --mode=0755 /tmp/bazel /usr/local/bin/bazel

WORKDIR /workspace/nearby
COPY nearby/ ./
COPY overrides/ /workspace/overrides/
COPY cache/repository/ /var/cache/bazel-repository/

RUN --mount=type=cache,target=/var/cache/bazel-disk \
    CC="/usr/bin/clang-${CLANG_VERSION}" \
    CXX="/usr/bin/clang++-${CLANG_VERSION}" \
    bazel --output_user_root=/var/cache/bazel-user build \
      --lockfile_mode=error \
      --repository_cache=/var/cache/bazel-repository \
      --disk_cache=/var/cache/bazel-disk \
      --action_env=CC \
      --action_env=CXX \
      --repo_env=CC \
      --repo_env=CXX \
      --override_repository=aappleby_smhasher=/workspace/overrides/smhasher \
      --override_repository=com_github_protobuf_matchers=/workspace/overrides\
/protobuf-matchers \
      --override_repository=com_google_nisaba=/workspace/overrides/nisaba \
      --override_repository=com_google_ukey2=/workspace/overrides/google-ukey2 \
      --override_repository=nlohmann_json=/workspace/overrides/nlohmann-json \
      --override_repository=sdbus_cpp=/workspace/overrides/sdbus-cpp \
      --@com_google_protobuf//bazel/toolchains:prefer_prebuilt_protoc=true \
      --copt=-DGITHUB_BUILD \
      //connections/file_share:file_share \
      //sharing/linux:nearby_sharing_cli

FROM ${UBUNTU_IMAGE} AS runtime

ARG CA_CERTIFICATES_PACKAGE
ARG UBUNTU_APT_SNAPSHOT_URL
ARG RUNTIME_PACKAGES

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes \
      --no-install-recommends ${CA_CERTIFICATES_PACKAGE} \
    && rm -rf /var/lib/apt/lists/*

RUN test -n "${UBUNTU_APT_SNAPSHOT_URL}" \
    && test -n "${RUNTIME_PACKAGES}" \
    && rm -f /etc/apt/sources.list.d/ubuntu.sources \
    && printf '%s\n' \
      "deb ${UBUNTU_APT_SNAPSHOT_URL} noble main universe" \
      "deb ${UBUNTU_APT_SNAPSHOT_URL} noble-updates main universe" \
      "deb ${UBUNTU_APT_SNAPSHOT_URL} noble-security main universe" \
      > /etc/apt/sources.list \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install --yes \
      --no-install-recommends ${RUNTIME_PACKAGES} \
    && groupadd --system quickshare \
    && useradd --system --gid quickshare --create-home \
      quickshare \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder \
  /workspace/nearby/bazel-bin/connections/file_share/file_share \
  /usr/local/bin/file_share
COPY --from=builder \
  /workspace/nearby/bazel-bin/sharing/linux/nearby_sharing_cli \
  /usr/local/bin/nearby_sharing_cli
COPY assets/NetworkManager.conf /etc/NetworkManager/NetworkManager.conf
COPY assets/dbus-1/system.d/org.bluez.conf /etc/dbus-1/system.d/org.bluez.conf
COPY assets/peer-entrypoint.sh /usr/local/libexec/nearby-linux-peer

RUN chmod 0755 /usr/local/bin/file_share \
    /usr/local/bin/nearby_sharing_cli \
    /usr/local/libexec/nearby-linux-peer

ARG ENVIRONMENT_FINGERPRINT
ENV DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/dbus/system_bus_socket
LABEL org.omarchy-quickshare.environment="nearby-linux" \
      org.omarchy-quickshare.fingerprint="${ENVIRONMENT_FINGERPRINT}"
ENTRYPOINT ["/usr/local/libexec/nearby-linux-peer"]
