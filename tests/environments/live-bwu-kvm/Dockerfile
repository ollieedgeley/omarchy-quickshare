# The guest kernel and BlueZ userspace come from the prepared radio image.
# The peer is copied with its Ubuntu runtime closure and always executed by
# that closure's loader: Debian's glibc is never asked to load Ubuntu's C++.
ARG NEARBY_IMAGE=omarchy-quickshare/nearby-linux-peer:2026-09-02
ARG RADIO_IMAGE=omarchy-quickshare/bluetooth-radio:2026-09-02
FROM ${NEARBY_IMAGE} AS nearby-runtime

RUN set -eu; \
    binary=/usr/local/bin/connections_peer; \
    loader=/lib64/ld-linux-x86-64.so.2; \
    test -x "$loader"; \
    mkdir -p /sealed/bin /sealed/root; \
    cp "$binary" /sealed/bin/connections_peer; \
    cp "$(readlink -f "$loader")" /sealed/loader; \
    ldd "$binary" | awk '$2 == "=>" { print $3 } $1 ~ /^\// { print $1 }' | \
      while read -r library; do \
        resolved="$(readlink -f "$library")"; \
        mkdir -p "/sealed/root$(dirname "$library")"; \
        cp "$resolved" "/sealed/root$library"; \
        mkdir -p "/sealed/root$(dirname "$resolved")"; \
        cp "$resolved" "/sealed/root$resolved"; \
      done; \
    test -x /sealed/loader

FROM ${RADIO_IMAGE} AS runtime

COPY --from=nearby-runtime /sealed/ /opt/nearby/

RUN /opt/nearby/loader --verify /opt/nearby/bin/connections_peer

ARG ENVIRONMENT_FINGERPRINT
LABEL org.omarchy-quickshare.environment="live-bwu-kvm" \
      org.omarchy-quickshare.fingerprint="${ENVIRONMENT_FINGERPRINT}"
